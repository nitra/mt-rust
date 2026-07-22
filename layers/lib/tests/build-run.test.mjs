import { describe, expect, test } from 'vitest'

import { bootstrapEssences, runBuild } from '../build.mjs'
import { extractEssence, parseDoc } from '../md.mjs'
import { computeStatus } from '../status.mjs'
import { leafDoc, memoryReader } from './helpers.mjs'

const TITLE_RE = /почни з рядка "# (.+?)"/
/**
 * Тихий лог для прогонів у тестах.
 * @param {string} text рядок прогресу
 * @returns {string} той самий рядок (ігнорується)
 */
const silentLog = text => text

/**
 * Стаб-LLM для build/bootstrap: складає валідний вихід із system-промпту.
 * @returns {{generate: (request: object) => Promise<{content: string, model: string}>, calls: object[]}} стаб
 */
function stubLlm() {
  const calls = []
  return {
    calls,
    /**
     * @param {object} request запит генерації
     * @returns {Promise<{content: string, model: string}>} валідний для запиту вихід
     */
    generate(request) {
      calls.push(request)
      const title = request.system.match(TITLE_RE)?.[1]
      let content = 'Просте резюме без заголовків і жаргону.'
      if (title) content = `# ${title}\n\n## Суть\n\nСтаб-суть огляду.\n\n## Розгортка\n\nАгрегований текст.`
      else if (request.system.includes('Сформулюй «суть»')) content = 'Чернетка суті документа.'
      if (request.validate && !request.validate(content)) {
        return Promise.reject(Object.assign(new Error('стаб не пройшов валідацію'), { code: 'output' }))
      }
      return Promise.resolve({ content, model: 'stub/model' })
    }
  }
}

/** @returns {object} конфіг полігона L2→L1→L0(fragment) над двома leaf */
function makeConfig() {
  return {
    docsDir: '/віртуальний',
    tier: 'avg',
    maxTokens: 4096,
    i18n: { baseLang: 'uk', langs: [] },
    docs: {
      'overview/core.md': { layer: 'L2', title: 'Ядро', sources: ['a.md', 'b.md'] },
      'overview/index.md': { layer: 'L1', title: 'Як це працює', sources: ['overview/core.md'] },
      'index.md': { layer: 'L0', mode: 'fragment', sources: ['overview/index.md'] }
    },
    leaves: ['a.md', 'b.md']
  }
}

/**
 * @param {Record<string, string>} files віртуальна ФС
 * @returns {{readFile: (rel: string) => string | null, writeFile: (rel: string, text: string) => void}} io-пара
 */
function ioOver(files) {
  return {
    readFile: memoryReader(files),
    /**
     * @param {string} rel шлях
     * @param {string} text вміст
     * @returns {void}
     */
    writeFile(rel, text) {
      files[rel] = text
    }
  }
}

const HOST =
  '# Індекс\n\n<!-- layers:L0 sources: overview/index.md 00000000 00000000 -->\nстаре\n<!-- /layers:L0 -->\n\n## Решта\n'

describe('runBuild', () => {
  test('будує знизу вгору: L2 → L1 → L0-fragment, полігон стає fresh', async () => {
    const files = {
      'a.md': leafDoc({ essence: 'Суть А.' }),
      'b.md': leafDoc({ essence: 'Суть Б.' }),
      'index.md': HOST
    }
    const llm = stubLlm()
    const result = await runBuild(makeConfig(), { io: ioOver(files), llm, log: silentLog, today: '2026-07-12' })
    expect(result.built).toEqual(['overview/core.md', 'overview/index.md', 'index.md'])
    expect(result.failed).toEqual([])

    const core = parseDoc(files['overview/core.md'])
    expect(core.fm.type).toBe('layered-doc')
    expect(core.fm.layers.model).toBe('stub/model')
    expect(core.fm.layers.sources).toHaveLength(2)
    expect(extractEssence(core.body)).not.toBeNull()
    expect(core.body).toContain('## Глибше')
    expect(core.body).toContain('](../a.md)')

    expect(files['index.md']).toContain('Просте резюме')
    expect(files['index.md']).toContain('Докладніше: [Як це працює](overview/index.md)')
    expect(files['index.md']).toContain('## Решта')

    const report = computeStatus(makeConfig(), memoryReader(files))
    expect(report.worst).toBe('fresh')
  })

  test('вихід моделі без порожнього рядка після заголовка нормалізується (MD022)', async () => {
    const files = {
      'a.md': leafDoc({ essence: 'Суть А.' }),
      'b.md': leafDoc({ essence: 'Суть Б.' }),
      'index.md': HOST
    }
    const llm = {
      /**
       * @param {object} request запит генерації
       * @returns {Promise<{content: string, model: string}>} вихід без порожніх рядків після заголовків
       */
      generate(request) {
        const title = request.system.match(TITLE_RE)?.[1]
        const content = `# ${title}\n## Суть\nСтаб-суть без відступів.\n## Розгортка\nТекст одразу після заголовка.`
        return Promise.resolve({ content, model: 'stub/model' })
      }
    }
    await runBuild(makeConfig(), { io: ioOver(files), llm, log: silentLog, today: '2026-07-12' })
    const { body } = parseDoc(files['overview/core.md'])
    expect(body).toContain('## Суть\n\nСтаб-суть без відступів.')
    expect(body).toContain('## Розгортка\n\nТекст одразу після заголовка.')
  })

  test('draft-суть джерела блокує доку, помилка пояснює що робити', async () => {
    const files = {
      'a.md': leafDoc({ essence: 'Суть А.', draft: true }),
      'b.md': leafDoc({ essence: 'Суть Б.' }),
      'index.md': HOST
    }
    const result = await runBuild(makeConfig(), {
      io: ioOver(files),
      llm: stubLlm(),
      log: silentLog,
      today: '2026-07-12'
    })
    expect(result.failed.find(f => f.file === 'overview/core.md').reason).toContain('essence:draft')
    expect(files['overview/core.md']).toBeUndefined()
  })

  test('dry-run: кандидати залоговані, файли не пишуться', async () => {
    const files = {
      'a.md': leafDoc({ essence: 'Суть А.' }),
      'b.md': leafDoc({ essence: 'Суть Б.' }),
      'index.md': HOST
    }
    const lines = []
    const llm = stubLlm()
    const result = await runBuild(makeConfig(), {
      io: ioOver(files),
      llm,
      dryRun: true,
      log: text => {
        lines.push(text)
      },
      today: '2026-07-12'
    })
    expect(result.built).toHaveLength(3)
    expect(llm.calls).toHaveLength(0)
    expect(files['overview/core.md']).toBeUndefined()
    expect(files['index.md']).toBe(HOST)
    expect(lines.join('\n')).toContain('[dry-run]')
  })

  test('свіжі доки пропускаються; ідемпотентність повторного build', async () => {
    const files = {
      'a.md': leafDoc({ essence: 'Суть А.' }),
      'b.md': leafDoc({ essence: 'Суть Б.' }),
      'index.md': HOST
    }
    const io = ioOver(files)
    await runBuild(makeConfig(), { io, llm: stubLlm(), log: silentLog, today: '2026-07-12' })
    const snapshot = { ...files }
    const second = await runBuild(makeConfig(), { io, llm: stubLlm(), log: silentLog, today: '2026-07-12' })
    expect(second.built).toEqual([])
    expect(second.skipped).toHaveLength(3)
    expect(files).toEqual(snapshot)
  })

  test('LLM недоступна → помилка пробивається нагору (exit 3 у cli)', async () => {
    const files = {
      'a.md': leafDoc({ essence: 'Суть А.' }),
      'b.md': leafDoc({ essence: 'Суть Б.' }),
      'index.md': HOST
    }
    const llm = {
      /**
       * @returns {Promise<never>} завжди відмова транспорту
       */
      generate() {
        return Promise.reject(Object.assign(new Error('LLM недоступна'), { code: 'unavailable' }))
      }
    }
    await expect(
      runBuild(makeConfig(), { io: ioOver(files), llm, log: silentLog, today: '2026-07-12' })
    ).rejects.toMatchObject({ code: 'unavailable' })
  })
})

describe('bootstrapEssences', () => {
  test('додає чернетки лише leaf-докам без суті; ідемпотентний', async () => {
    const files = {
      'a.md': leafDoc({ essence: null }),
      'b.md': leafDoc({ essence: 'Готова суть.' })
    }
    const config = makeConfig()
    const io = ioOver(files)
    const first = await bootstrapEssences(config, { io, llm: stubLlm(), log: silentLog })
    expect(first.drafted).toEqual(['a.md'])
    expect(first.skipped).toEqual(['b.md'])
    const essence = extractEssence(parseDoc(files['a.md']).body)
    expect(essence).toEqual({ text: 'Чернетка суті документа.', draft: true })
    // frontmatter leaf-доки збережений
    expect(parseDoc(files['a.md']).fm.type).toBe('architecture')

    const second = await bootstrapEssences(config, { io, llm: stubLlm(), log: silentLog })
    expect(second.drafted).toEqual([])
    expect(second.skipped).toEqual(['a.md', 'b.md'])
  })
})
