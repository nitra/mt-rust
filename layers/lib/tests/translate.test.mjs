import { describe, expect, test } from 'vitest'

import { currentCrcs } from '../build.mjs'
import { parseDoc } from '../md.mjs'
import { rewriteLinks, runTranslate, splitBySections } from '../translate.mjs'
import { leafDoc, memoryReader } from './helpers.mjs'

/**
 * Тихий лог для прогонів у тестах.
 * @param {string} text рядок прогресу
 * @returns {string} той самий рядок (ігнорується)
 */
const silentLog = text => text

/**
 * Identity-стаб перекладача: повертає вхід без змін (структура збережена).
 * @param {{transform?: (chunk: string) => string}} [options] трансформація «перекладу»
 * @returns {{generate: (request: object) => Promise<{content: string, model: string}>, calls: object[]}} стаб
 */
function stubLlm({ transform } = {}) {
  const calls = []
  return {
    calls,
    /**
     * @param {object} request запит генерації
     * @returns {Promise<{content: string, model: string}>} «переклад» фрагмента
     */
    generate(request) {
      calls.push(request)
      const content = transform ? transform(request.user) : request.user
      if (request.validate && !request.validate(content)) {
        return Promise.reject(Object.assign(new Error('структура не збережена'), { code: 'output' }))
      }
      return Promise.resolve({ content, model: 'stub/translator' })
    }
  }
}

/** @returns {object} конфіг з одним L2 і мовою en */
function makeConfig() {
  return {
    docsDir: '/віртуальний',
    tier: 'avg',
    maxTokens: 4096,
    i18n: { baseLang: 'uk', langs: ['en'] },
    docs: { 'overview/core.md': { layer: 'L2', title: 'Ядро', sources: ['a.md', 'b.md'] } },
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

function freshFiles() {
  return {
    'a.md': leafDoc({
      essence: 'Суть А.',
      details: 'Деталі з [лінком на Б](b.md) і [зовнішнім](https://x.test/y.md).'
    }),
    'b.md': leafDoc({ essence: 'Суть Б.' })
  }
}

describe('runTranslate', () => {
  test('генерує derived-переклади з правильним frontmatter і мовними лінками', async () => {
    const files = freshFiles()
    const result = await runTranslate(makeConfig(), {
      io: ioOver(files),
      llm: stubLlm(),
      log: silentLog,
      today: '2026-07-12'
    })
    expect(result.written).toEqual(['a.en.md', 'b.en.md'])
    const { fm, body } = parseDoc(files['a.en.md'])
    expect(fm).toMatchObject({
      type: 'layered-translation',
      source: 'a.md',
      lang: 'en',
      authored: false,
      model: 'stub/translator'
    })
    expect(fm.sourceFileCrc).toBe(currentCrcs(files['a.md']).fileCrc)
    expect(body).toContain('](b.en.md)') // ціль у scope → мовний файл
    expect(body).toContain('https://x.test/y.md') // зовнішній лінк недоторканий
  })

  test('свіжий переклад пропускається; зміна base → регенерація', async () => {
    const files = freshFiles()
    const io = ioOver(files)
    const deps = { io, llm: stubLlm(), log: silentLog, today: '2026-07-12' }
    await runTranslate(makeConfig(), deps)
    const second = await runTranslate(makeConfig(), deps)
    expect(second.written).toEqual([])
    expect(second.skipped).toEqual(['a.en.md', 'b.en.md'])

    files['a.md'] = leafDoc({ essence: 'Суть А.', details: 'Нові деталі.' })
    const third = await runTranslate(makeConfig(), deps)
    expect(third.written).toEqual(['a.en.md'])
  })

  test('authored-переклад: при незмінному base не чіпається, при зміні — регенерація з попередженням', async () => {
    const files = freshFiles()
    const io = ioOver(files)
    await runTranslate(makeConfig(), { io, llm: stubLlm(), log: silentLog, today: '2026-07-12' })
    files['b.en.md'] = files['b.en.md'].replace('authored: false', 'authored: true')

    const warnings = []
    const deps = {
      io,
      llm: stubLlm(),
      log: text => {
        warnings.push(text)
      },
      today: '2026-07-12'
    }
    const untouched = await runTranslate(makeConfig(), deps)
    expect(untouched.skipped).toContain('b.en.md')

    files['b.md'] = leafDoc({ essence: 'Суть Б.', details: 'База змінилась.' })
    const regenerated = await runTranslate(makeConfig(), deps)
    expect(regenerated.written).toEqual(['b.en.md'])
    expect(warnings.join('\n')).toContain('authored-переклад застарів')
  })

  test('переклад, що губить лінки, фейлиться валідацією і не пишеться', async () => {
    const files = freshFiles()
    const result = await runTranslate(makeConfig(), {
      io: ioOver(files),
      llm: stubLlm({ transform: chunk => chunk.replaceAll(/\]\([^)]*\)/g, '](зникло)') }),
      only: 'a.md',
      log: silentLog,
      today: '2026-07-12'
    })
    expect(result.failed.map(f => f.file)).toEqual(['a.en.md'])
    expect(files['a.en.md']).toBeUndefined()
  })

  test('dry-run нічого не пише; --lang фільтрує мови', async () => {
    const files = freshFiles()
    const result = await runTranslate(makeConfig(), {
      io: ioOver(files),
      llm: stubLlm(),
      dryRun: true,
      lang: 'en',
      log: silentLog,
      today: '2026-07-12'
    })
    expect(result.written).toEqual(['a.en.md', 'b.en.md'])
    expect(files['a.en.md']).toBeUndefined()
  })
})

describe('splitBySections', () => {
  test('коротке тіло — один фрагмент', () => {
    expect(splitBySections('# X\n\nтекст\n')).toEqual(['# X\n\nтекст\n'])
  })

  test('довге тіло ділиться по межах H2, конкатенація відтворює оригінал', () => {
    const section = `## Секція\n\n${'слово '.repeat(1200)}\n\n`
    const body = `# Довга дока\n\n${section}${section}${section}`
    const chunks = splitBySections(body)
    expect(chunks.length).toBeGreaterThan(1)
    expect(chunks.join('')).toBe(body)
    for (const chunk of chunks.slice(1)) expect(chunk.startsWith('## ')).toBe(true)
  })
})

describe('rewriteLinks', () => {
  test('відносні цілі у scope отримують мовний суфікс, якорі зберігаються', () => {
    const scope = new Set(['a.md', 'architecture/graph.md'])
    const text = 'Див. [A](../a.md#секція) і [G](graph.md), але не [чуже](other.md).'
    const rewritten = rewriteLinks(text, 'architecture/overview.md', 'en', scope)
    expect(rewritten).toContain('](../a.en.md#секція)')
    expect(rewritten).toContain('](graph.en.md)')
    expect(rewritten).toContain('](other.md)')
  })
})
