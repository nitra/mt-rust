import { mkdirSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'

import { describe, expect, test } from 'vitest'

import { main } from '../cli.mjs'
import { generatedDoc, leafDoc, withTmpDir } from './helpers.mjs'

/**
 * Розгортає мінімальний полігон: два leaf + конфіг з одним L2.
 * @param {string} dir тека полігона
 * @param {{withGenerated?: boolean}} [options] чи класти згенеровану L2-доку
 * @returns {void}
 */
function scaffold(dir, { withGenerated = true } = {}) {
  const a = leafDoc({ essence: 'Суть А.' })
  const b = leafDoc({ essence: 'Суть Б.' })
  writeFileSync(join(dir, 'a.md'), a)
  writeFileSync(join(dir, 'b.md'), b)
  mkdirSync(join(dir, 'overview'), { recursive: true })
  if (withGenerated) {
    writeFileSync(
      join(dir, 'overview/core.md'),
      generatedDoc({ layer: 'L2', title: 'Ядро', sources: { 'a.md': a, 'b.md': b } })
    )
  }
  writeFileSync(
    join(dir, 'layers.json'),
    JSON.stringify({
      version: 1,
      docs: { 'overview/core.md': { layer: 'L2', title: 'Ядро', sources: ['a.md', 'b.md'] } }
    })
  )
}

/**
 * Збирає лог CLI у масив.
 * @returns {{lines: string[], log: (text: string) => void}} акумулятор і колбек логування
 */
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

describe('cli', () => {
  test('status на свіжому полігоні → exit 0', async () => {
    await withTmpDir(async dir => {
      scaffold(dir)
      const out = collector()
      expect(await main(['status', dir], out)).toBe(0)
      expect(out.lines.join('\n')).toContain('Підсумок: fresh')
    })
  })

  test('status: відсутня згенерована дока → exit 1, --json валідний', async () => {
    await withTmpDir(async dir => {
      scaffold(dir, { withGenerated: false })
      const out = collector()
      expect(await main(['status', dir, '--json'], out)).toBe(1)
      expect(JSON.parse(out.lines[0]).worst).toBe('stale')
    })
  })

  test('refresh після details-only дрейфу повертає полігон у fresh', async () => {
    await withTmpDir(async dir => {
      scaffold(dir)
      writeFileSync(join(dir, 'a.md'), leafDoc({ essence: 'Суть А.', details: 'Дрейф деталей.' }))
      const out = collector()
      expect(await main(['refresh', dir], out)).toBe(0)
      expect(out.lines.join('\n')).toContain('overview/core.md')
      expect(await main(['status', dir], collector())).toBe(0)
    })
  })

  test('тека без layers.json → exit 2', async () => {
    await withTmpDir(async dir => {
      const out = collector()
      expect(await main(['status', dir], out)).toBe(2)
      expect(out.lines.join('\n')).toContain('не тека шарової документації')
    })
  })

  test('невідома команда → exit 2 з usage; без аргументів → usage, exit 0', async () => {
    await withTmpDir(async dir => {
      scaffold(dir)
      expect(await main(['вигадка', dir], collector())).toBe(2)
      const out = collector()
      expect(await main([], out)).toBe(0)
      expect(out.lines.join('\n')).toContain('Використання')
    })
  })
})
