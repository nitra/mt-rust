import { writeFileSync } from 'node:fs'
import { join } from 'node:path'

import { describe, expect, test } from 'vitest'

import { langPath, layerNumber, loadConfig, topoOrder, translationScope } from '../layers.mjs'
import { withTmpDir } from './helpers.mjs'

const VALID_CONFIG = {
  version: 1,
  tier: 'avg',
  i18n: { baseLang: 'uk', langs: ['en'] },
  docs: {
    'overview/core.md': { layer: 'L2', title: 'Ядро', sources: ['a.md', 'b.md'] },
    'overview/index.md': { layer: 'L1', title: 'Як це працює', sources: ['overview/core.md'] },
    'index.md': { layer: 'L0', mode: 'fragment', sources: ['overview/index.md'] }
  }
}

const NO_CONFIG_RE = /не тека шарової документації/
const WRONG_DIRECTION_RE = /не з нижчого шару/
const SELF_REFERENCE_RE = /сам на себе/
const FRAGMENT_AS_SOURCE_RE = /fragment-дока не може бути джерелом/
const INVALID_LAYER_RE = /невалідна мітка шару[\s\S]*порожній список/

/**
 * @param {string} dir тека полігона
 * @param {object} config вміст layers.json
 */
function writeConfig(dir, config) {
  writeFileSync(join(dir, 'layers.json'), JSON.stringify(config))
}

describe('loadConfig', () => {
  test('валідний конфіг: leaves виводяться, дефолти застосовуються', async () => {
    await withTmpDir(dir => {
      writeConfig(dir, VALID_CONFIG)
      const config = loadConfig(dir)
      expect(config.leaves).toEqual(['a.md', 'b.md'])
      expect(config.maxTokens).toBe(4096)
      expect(config.i18n).toEqual({ baseLang: 'uk', langs: ['en'] })
    })
  })

  test('нема layers.json → зрозуміла помилка', async () => {
    await withTmpDir(dir => {
      expect(() => loadConfig(dir)).toThrow(NO_CONFIG_RE)
    })
  })

  test('джерело з того самого шару → помилка напрямку', async () => {
    await withTmpDir(dir => {
      writeConfig(dir, {
        docs: {
          'x.md': { layer: 'L1', sources: ['y.md'] },
          'y.md': { layer: 'L1', sources: ['leaf.md'] }
        }
      })
      expect(() => loadConfig(dir)).toThrow(WRONG_DIRECTION_RE)
    })
  })

  test('самопосилання → помилка', async () => {
    await withTmpDir(dir => {
      writeConfig(dir, { docs: { 'x.md': { layer: 'L1', sources: ['x.md'] } } })
      expect(() => loadConfig(dir)).toThrow(SELF_REFERENCE_RE)
    })
  })

  test('fragment-дока як джерело іншої → помилка', async () => {
    await withTmpDir(dir => {
      writeConfig(dir, {
        docs: {
          'index.md': { layer: 'L1', mode: 'fragment', sources: ['leaf.md'] },
          'top.md': { layer: 'L0', sources: ['index.md'] }
        }
      })
      expect(() => loadConfig(dir)).toThrow(FRAGMENT_AS_SOURCE_RE)
    })
  })

  test('порожні sources та невалідна мітка шару → помилки', async () => {
    await withTmpDir(dir => {
      writeConfig(dir, { docs: { 'x.md': { layer: 'горішній', sources: [] } } })
      expect(() => loadConfig(dir)).toThrow(INVALID_LAYER_RE)
    })
  })
})

describe('утиліти топології', () => {
  test('layerNumber парсить L<n>, інше → NaN', () => {
    expect(layerNumber('L2')).toBe(2)
    expect(layerNumber('L0')).toBe(0)
    expect(Number.isNaN(layerNumber('шар'))).toBe(true)
  })

  test('topoOrder — знизу вгору (L2 → L1 → L0)', async () => {
    await withTmpDir(dir => {
      writeConfig(dir, VALID_CONFIG)
      expect(topoOrder(loadConfig(dir))).toEqual(['overview/core.md', 'overview/index.md', 'index.md'])
    })
  })

  test('translationScope = leaves + всі доки конфігу', async () => {
    await withTmpDir(dir => {
      writeConfig(dir, VALID_CONFIG)
      expect(translationScope(loadConfig(dir))).toEqual([
        'a.md',
        'b.md',
        'overview/core.md',
        'overview/index.md',
        'index.md'
      ])
    })
  })

  test('langPath додає суфікс мови перед .md', () => {
    expect(langPath('architecture/graph.md', 'en')).toBe('architecture/graph.en.md')
  })
})
