import { describe, expect, test } from 'vitest'

import { currentCrcs } from '../build.mjs'
import { formatSourceLine } from '../md.mjs'
import { computeStatus, renderStatus, statusExitCode } from '../status.mjs'
import { generatedDoc, leafDoc, memoryReader } from './helpers.mjs'

const DETAILS_REFRESH_RE = /refresh/
const NEEDS_BUILD_RE = /потрібен build/
const MISSING_FROM_CONFIG_RE = /b\.md.*нема в конфігу/
const MD_EXT_RE = /\.md$/

/**
 * Мінімальний конфіг з одним L2 над двома leaf.
 * @param {object} [docsOverride] заміна секції docs (дефолт — один L2-запис)
 * @returns {object} конфіг полігона для computeStatus
 */
function makeConfig(docsOverride) {
  return {
    docsDir: '/віртуальний',
    tier: 'avg',
    maxTokens: 4096,
    i18n: { baseLang: 'uk', langs: [] },
    docs: docsOverride ?? {
      'overview/core.md': { layer: 'L2', title: 'Ядро', sources: ['a.md', 'b.md'] }
    },
    leaves: ['a.md', 'b.md']
  }
}

/**
 * Стандартна пара свіжих leaf + узгоджена L2-дока.
 * @returns {Record<string, string>} мапа шлях → вміст файлів полігона
 */
function freshFiles() {
  const a = leafDoc({ essence: 'Суть А.' })
  const b = leafDoc({ essence: 'Суть Б.' })
  return {
    'a.md': a,
    'b.md': b,
    'overview/core.md': generatedDoc({ layer: 'L2', title: 'Ядро', sources: { 'a.md': a, 'b.md': b } })
  }
}

/**
 * @param {ReturnType<typeof computeStatus>} report звіт computeStatus
 * @param {string} [file] шлях доки для пошуку
 * @returns {object | undefined} запис доки зі звіту
 */
function docState(report, file = 'overview/core.md') {
  return report.docs.find(doc => doc.file === file)
}

/**
 * Переклад-фікстура для `file`: узгоджений або розсинхронізований sourceFileCrc.
 * @param {Record<string, string>} files мапа шлях → вміст (для обчислення поточного CRC)
 * @param {string} file шлях base-файлу перекладу
 * @param {{authored?: boolean, crcOverride?: string}} [options] прапор authored і override CRC
 * @returns {string} вміст файлу перекладу
 */
function translationOf(files, file, { authored = false, crcOverride } = {}) {
  const crc = crcOverride ?? currentCrcs(files[file]).fileCrc
  return `---\ntype: layered-translation\nsource: ${file}\nlang: en\nsourceFileCrc: ${crc}\nauthored: ${authored}\n---\n\n# Translated\n`
}

describe('computeStatus: матриця станів джерела', () => {
  test('усе збігається → fresh, exit 0', () => {
    const report = computeStatus(makeConfig(), memoryReader(freshFiles()))
    expect(docState(report).state).toBe('fresh')
    expect(report.worst).toBe('fresh')
    expect(statusExitCode(report, false)).toBe(0)
  })

  test('змінились лише деталі → details-only; exit 0, зі --strict → 1', () => {
    const files = freshFiles()
    files['a.md'] = leafDoc({ essence: 'Суть А.', details: 'Інші деталі, суть та сама.' })
    const report = computeStatus(makeConfig(), memoryReader(files))
    expect(docState(report).state).toBe('details-only')
    expect(docState(report).sources.find(s => s.file === 'a.md').note).toMatch(DETAILS_REFRESH_RE)
    expect(statusExitCode(report, false)).toBe(0)
    expect(statusExitCode(report, true)).toBe(1)
  })

  test('змінилась суть → stale, exit 1', () => {
    const files = freshFiles()
    files['a.md'] = leafDoc({ essence: 'Зовсім нова суть.' })
    const report = computeStatus(makeConfig(), memoryReader(files))
    expect(docState(report).state).toBe('stale')
    expect(statusExitCode(report, false)).toBe(1)
  })

  test('джерело зникло → stale', () => {
    const files = freshFiles()
    delete files['a.md']
    const report = computeStatus(makeConfig(), memoryReader(files))
    expect(docState(report).sources.find(s => s.file === 'a.md').state).toBe('stale')
  })

  test('джерело без «## Суть» → no-essence, exit 2', () => {
    const files = freshFiles()
    files['a.md'] = leafDoc({ essence: null })
    const report = computeStatus(makeConfig(), memoryReader(files))
    expect(docState(report).state).toBe('no-essence')
    expect(statusExitCode(report, false)).toBe(2)
  })

  test('суть-чернетка → draft, exit 1', () => {
    const files = freshFiles()
    files['a.md'] = leafDoc({ essence: 'Суть А.', draft: true })
    const report = computeStatus(makeConfig(), memoryReader(files))
    expect(docState(report).state).toBe('draft')
    expect(statusExitCode(report, false)).toBe(1)
  })

  test('доки верхнього шару ще нема → stale з причиною', () => {
    const files = freshFiles()
    delete files['overview/core.md']
    const report = computeStatus(makeConfig(), memoryReader(files))
    expect(docState(report).state).toBe('stale')
    expect(docState(report).reasons.join(' ')).toMatch(NEEDS_BUILD_RE)
  })

  test('нове джерело в конфігу, не враховане в доці → stale', () => {
    const files = freshFiles()
    const a = files['a.md']
    files['overview/core.md'] = generatedDoc({ layer: 'L2', title: 'Ядро', sources: { 'a.md': a } })
    const report = computeStatus(makeConfig(), memoryReader(files))
    expect(docState(report).sources.find(s => s.file === 'b.md').state).toBe('stale')
  })

  test('у доці записане джерело, прибране з конфігу → stale з причиною', () => {
    const files = freshFiles()
    const config = makeConfig({
      'overview/core.md': { layer: 'L2', title: 'Ядро', sources: ['a.md'] }
    })
    config.leaves = ['a.md']
    const report = computeStatus(config, memoryReader(files))
    expect(docState(report).state).toBe('stale')
    expect(docState(report).reasons.join(' ')).toMatch(MISSING_FROM_CONFIG_RE)
  })

  test('пріоритет: no-essence перекриває stale і details-only', () => {
    const files = freshFiles()
    files['a.md'] = leafDoc({ essence: null })
    files['b.md'] = leafDoc({ essence: 'Нова суть Б.' })
    const report = computeStatus(makeConfig(), memoryReader(files))
    expect(docState(report).state).toBe('no-essence')
  })
})

describe('computeStatus: fragment-доки', () => {
  const FRAGMENT_CONFIG = {
    ...makeConfig({
      'overview/core.md': { layer: 'L2', title: 'Ядро', sources: ['a.md', 'b.md'] },
      'index.md': { layer: 'L0', mode: 'fragment', sources: ['overview/core.md'] }
    }),
    leaves: ['a.md', 'b.md']
  }

  test('fragment зі свіжими CRC у маркері → fresh', () => {
    const files = freshFiles()
    const core = files['overview/core.md']
    const line = formatSourceLine({ file: 'overview/core.md', ...currentCrcs(core) })
    files['index.md'] = `# Індекс\n\n<!-- layers:L0 sources: ${line} -->\nРезюме.\n<!-- /layers:L0 -->\n`
    const report = computeStatus(FRAGMENT_CONFIG, memoryReader(files))
    expect(docState(report, 'index.md').state).toBe('fresh')
  })

  test('fragment без маркерів → stale (джерело не враховане)', () => {
    const files = freshFiles()
    files['index.md'] = '# Індекс без маркерів\n'
    const report = computeStatus(FRAGMENT_CONFIG, memoryReader(files))
    expect(docState(report, 'index.md').state).toBe('stale')
  })
})

describe('computeStatus: переклади', () => {
  const I18N_CONFIG = { ...makeConfig(), i18n: { baseLang: 'uk', langs: ['en'] } }

  test('нема перекладу → missing, translationsPending, exit 1', () => {
    const report = computeStatus(I18N_CONFIG, memoryReader(freshFiles()))
    expect(report.translations.every(t => t.state === 'missing')).toBe(true)
    expect(report.translationsPending).toBe(true)
    expect(statusExitCode(report, false)).toBe(1)
  })

  test('переклади з актуальним sourceFileCrc → fresh, exit 0', () => {
    const files = freshFiles()
    for (const file of ['a.md', 'b.md', 'overview/core.md']) {
      files[file.replace(MD_EXT_RE, '.en.md')] = translationOf(files, file)
    }
    const report = computeStatus(I18N_CONFIG, memoryReader(files))
    expect(report.translations.every(t => t.state === 'fresh')).toBe(true)
    expect(statusExitCode(report, false)).toBe(0)
  })

  test('base змінився → переклад stale; authored при незмінному base → authored', () => {
    const files = freshFiles()
    files['a.en.md'] = translationOf(files, 'a.md', { crcOverride: 'deadbeef' })
    files['b.en.md'] = translationOf(files, 'b.md', { authored: true })
    files['overview/core.en.md'] = translationOf(files, 'overview/core.md')
    const report = computeStatus(I18N_CONFIG, memoryReader(files))
    const byFile = Object.fromEntries(report.translations.map(t => [t.file, t.state]))
    expect(byFile['a.en.md']).toBe('stale')
    expect(byFile['b.en.md']).toBe('authored')
    expect(byFile['overview/core.en.md']).toBe('fresh')
  })
})

describe('renderStatus', () => {
  test('--json віддає повний звіт, exit code збігається', () => {
    const report = computeStatus(makeConfig(), memoryReader(freshFiles()))
    const { text, exitCode } = renderStatus(report, { json: true })
    expect(JSON.parse(text).worst).toBe('fresh')
    expect(exitCode).toBe(0)
  })

  test('людиночитний звіт містить шар, стан і підсумок', () => {
    const files = freshFiles()
    files['a.md'] = leafDoc({ essence: 'Нова суть.' })
    const { text, exitCode } = renderStatus(computeStatus(makeConfig(), memoryReader(files)))
    expect(text).toContain('[L2] overview/core.md — stale')
    expect(text).toContain('суть джерела змінилась')
    expect(text).toContain('Підсумок: stale')
    expect(exitCode).toBe(1)
  })
})
