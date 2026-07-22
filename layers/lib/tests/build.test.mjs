import { describe, expect, test } from 'vitest'

import { currentCrcs, refreshFileCrcs } from '../build.mjs'
import { formatSourceLine, parseDoc, parseSourceLine } from '../md.mjs'
import { computeStatus } from '../status.mjs'
import { generatedDoc, leafDoc, memoryReader } from './helpers.mjs'

const MARKER_SOURCES_RE = /sources: (.*?) -->/

/**
 * Мінімальний конфіг: один L2 над двома leaf + fragment-хост L0.
 * @returns {object} конфіг полігона
 */
function makeConfig() {
  return {
    docsDir: '/віртуальний',
    tier: 'avg',
    maxTokens: 4096,
    i18n: { baseLang: 'uk', langs: [] },
    docs: {
      'overview/core.md': { layer: 'L2', title: 'Ядро', sources: ['a.md', 'b.md'] },
      'index.md': { layer: 'L0', mode: 'fragment', sources: ['overview/core.md'] }
    },
    leaves: ['a.md', 'b.md']
  }
}

/**
 * @param {Record<string, string>} files мапа шлях → вміст (мутується writeFile)
 * @returns {{readFile: (rel: string) => string | null, writeFile: (rel: string, text: string) => void}} io-пара поверх files
 */
function ioOver(files) {
  return {
    readFile: memoryReader(files),
    /**
     * @param {string} rel відносний шлях
     * @param {string} text новий вміст
     * @returns {void}
     */
    writeFile(rel, text) {
      files[rel] = text
    }
  }
}

/**
 * Стандартний свіжий полігон: два leaf, L2-дока, fragment-хост L0.
 * @returns {Record<string, string>} мапа шлях → вміст
 */
function freshFiles() {
  const a = leafDoc({ essence: 'Суть А.' })
  const b = leafDoc({ essence: 'Суть Б.' })
  const core = generatedDoc({ layer: 'L2', title: 'Ядро', sources: { 'a.md': a, 'b.md': b } })
  const line = formatSourceLine({ file: 'overview/core.md', ...currentCrcs(core) })
  return {
    'a.md': a,
    'b.md': b,
    'overview/core.md': core,
    'index.md': `# Індекс\n\n<!-- layers:L0 sources: ${line} -->\nРезюме.\n<!-- /layers:L0 -->\n`
  }
}

describe('refreshFileCrcs', () => {
  test('details-only: переписує лише fileCrc, дока стає fresh', () => {
    const files = freshFiles()
    files['a.md'] = leafDoc({ essence: 'Суть А.', details: 'Нові деталі.' })
    const updated = refreshFileCrcs(makeConfig(), ioOver(files))
    expect(updated).toEqual(['overview/core.md'])
    const report = computeStatus(makeConfig(), memoryReader(files))
    expect(report.worst).toBe('fresh')
  })

  test('stale-джерело не чіпається — це робота для build', () => {
    const files = freshFiles()
    const before = parseDoc(files['overview/core.md']).fm.layers.sources
    files['a.md'] = leafDoc({ essence: 'Інша суть.' })
    const updated = refreshFileCrcs(makeConfig(), ioOver(files))
    expect(updated).toEqual([])
    expect(parseDoc(files['overview/core.md']).fm.layers.sources).toEqual(before)
  })

  test('усе fresh → нічого не пишеться (ідемпотентність)', () => {
    const files = freshFiles()
    expect(refreshFileCrcs(makeConfig(), ioOver(files))).toEqual([])
  })

  test('оновлення frontmatter доки не каскадить: її fileCrc рахується від тіла', () => {
    const files = freshFiles()
    files['a.md'] = leafDoc({ essence: 'Суть А.', details: 'Нові деталі.' })
    expect(refreshFileCrcs(makeConfig(), ioOver(files))).toEqual(['overview/core.md'])
    // другий прохід: маркер index.md досі свіжий, бо тіло core не змінилось
    expect(refreshFileCrcs(makeConfig(), ioOver(files))).toEqual([])
  })

  test('fragment: дрейф тіла джерела оновлює маркер, вміст і решта файлу недоторкані', () => {
    const files = freshFiles()
    files['overview/core.md'] = files['overview/core.md'].replace('Текст.', 'Інший текст, суть та сама.')
    const updated = refreshFileCrcs(makeConfig(), ioOver(files))
    expect(updated).toEqual(['index.md'])
    expect(files['index.md']).toContain('Резюме.')
    expect(files['index.md']).toContain('# Індекс')
    const marker = files['index.md'].match(MARKER_SOURCES_RE)[1]
    expect(parseSourceLine(marker).fileCrc).toBe(currentCrcs(files['overview/core.md']).fileCrc)
  })
})
