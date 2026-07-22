import { writeFileSync } from 'node:fs'
import { join } from 'node:path'

import { describe, expect, test } from 'vitest'

import { main } from '../cli.mjs'
import { leafDoc, withTmpDir } from './helpers.mjs'

const TITLE_RE = /почни з рядка "# (.+?)"/

/**
 * Injected-транспорт для createLlm: складає валідний вихід із system-промпту.
 * @param {{fail?: boolean}} [options] fail — імітувати недоступний транспорт
 * @returns {{runOneShot: (request: object) => Promise<object>}} impl для createLlm
 */
function transportStub({ fail = false } = {}) {
  return {
    /**
     * @param {object} request запит one-shot
     * @returns {Promise<object>} відповідь стаба
     */
    runOneShot(request) {
      if (fail) return Promise.resolve({ error: 'ECONNREFUSED 127.0.0.1:8000' })
      const system = request.messages[0].content
      const title = system.match(TITLE_RE)?.[1]
      let content = 'Просте резюме без заголовків.'
      if (title) content = `# ${title}\n\n## Суть\n\nСуть огляду.\n\n## Розгортка\n\nТекст.`
      else if (system.includes('Сформулюй «суть»')) content = 'Чернетка суті.'
      else if (system.includes('перекладач')) content = request.messages[1].content
      return Promise.resolve({ content, model: 'stub/model' })
    }
  }
}

/**
 * Мінімальний полігон: два leaf із сутями, один L2, мова en.
 * @param {string} dir тимчасова тека
 */
function scaffold(dir) {
  writeFileSync(join(dir, 'a.md'), leafDoc({ essence: 'Суть А.' }))
  writeFileSync(join(dir, 'b.md'), leafDoc({ essence: 'Суть Б.' }))
  writeFileSync(
    join(dir, 'layers.json'),
    JSON.stringify({
      version: 1,
      i18n: { baseLang: 'uk', langs: ['en'] },
      docs: { 'core.md': { layer: 'L2', title: 'Ядро', sources: ['a.md', 'b.md'] } }
    })
  )
}

/** @returns {{lines: string[], log: (text: string) => void}} колектор виводу */
function collector() {
  /** @type {string[]} */
  const lines = []
  return {
    lines,
    log: text => {
      lines.push(text)
    }
  }
}

describe('cli: LLM-команди', () => {
  test('build → translate → status: полігон доходить до fresh, exit 0', async () => {
    await withTmpDir(async dir => {
      scaffold(dir)
      const llmImpl = transportStub()
      expect(await main(['build', dir], { ...collector(), llmImpl })).toBe(0)
      expect(await main(['status', dir], collector())).toBe(1) // переклади ще missing
      expect(await main(['translate', dir], { ...collector(), llmImpl })).toBe(0)
      expect(await main(['status', dir], collector())).toBe(0)
    })
  })

  test('LLM недоступна → exit 3, файли не чіпаються', async () => {
    await withTmpDir(async dir => {
      scaffold(dir)
      const out = collector()
      expect(await main(['build', dir], { ...out, llmImpl: transportStub({ fail: true }) })).toBe(3)
      expect(out.lines.join('\n')).toContain('не вдався')
      expect(await main(['status', dir, '--json'], collector())).toBe(1) // core.md так і не збудований
    })
  })

  test('build створює вкладену теку призначення, якщо вона ще не існує', async () => {
    await withTmpDir(async dir => {
      writeFileSync(join(dir, 'a.md'), leafDoc({ essence: 'Суть А.' }))
      writeFileSync(join(dir, 'b.md'), leafDoc({ essence: 'Суть Б.' }))
      writeFileSync(
        join(dir, 'layers.json'),
        JSON.stringify({
          version: 1,
          docs: { 'overview/core.md': { layer: 'L2', title: 'Ядро', sources: ['a.md', 'b.md'] } }
        })
      )
      const result = await main(['build', dir], { ...collector(), llmImpl: transportStub() })
      expect(result).toBe(0)
    })
  })

  test('bootstrap: генерує чернетку суті для leaf без секції', async () => {
    await withTmpDir(async dir => {
      scaffold(dir)
      writeFileSync(join(dir, 'a.md'), leafDoc({ essence: null }))
      const out = collector()
      expect(await main(['bootstrap', dir], { ...out, llmImpl: transportStub() })).toBe(0)
      expect(out.lines.join('\n')).toContain('Чернеток: 1')
      // чернетка блокує build до рев'ю
      expect(await main(['status', dir], collector())).toBe(1)
    })
  })
})
