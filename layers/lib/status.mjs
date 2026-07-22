/** @see ./docs/status.md */

import { essenceCrc, fileCrc } from './crc.mjs'
import { langPath, topoOrder, translationScope } from './layers.mjs'
import { extractEssence, extractFragment, parseDoc, parseSourceLine } from './md.mjs'

/** Порядок суворості станів: індекс більший — стан гірший. */
const SEVERITY = ['fresh', 'details-only', 'draft', 'stale', 'no-essence']

/**
 * @param {string[]} states список станів для згортання в найгірший
 * @returns {string} найгірший (найсуворіший) стан зі списку
 */
function worstOf(states) {
  let worst = 'fresh'
  for (const state of states) {
    if (SEVERITY.indexOf(state) > SEVERITY.indexOf(worst)) worst = state
  }
  return worst
}

/**
 * Поточні CRC і стан суті одного файлу.
 * @param {string | null} text вміст файлу або null, якщо його нема
 * @returns {{fileCrc: string, essenceCrc: string | null, draft: boolean} | null} стан файлу або null, якщо файлу нема
 */
function inspectSource(text) {
  if (text === null) return null
  const { body } = parseDoc(text)
  const essence = extractEssence(body)
  return {
    fileCrc: fileCrc(body),
    essenceCrc: essence ? essenceCrc(essence.text) : null,
    draft: essence?.draft ?? false
  }
}

/**
 * Записані у доці пари CRC джерел: із frontmatter `layers.sources`
 * або з fragment-маркера.
 * @param {string | null} text вміст доки або null, якщо її нема
 * @param {boolean} isFragment true — дока fragment-режиму
 * @returns {Map<string, {essenceCrc: string, fileCrc: string}> | null} null — доки нема
 */
function recordedSources(text, isFragment) {
  if (text === null) return null
  const map = new Map()
  if (isFragment) {
    const fragment = extractFragment(text)
    if (!fragment) return map
    for (const source of fragment.sources) map.set(source.file, source)
    return map
  }
  const { fm } = parseDoc(text)
  const lines = fm?.layers?.sources
  if (!Array.isArray(lines)) return map
  for (const line of lines) {
    const source = parseSourceLine(String(line))
    map.set(source.file, source)
  }
  return map
}

/**
 * Стан перекладу відносно base-версії: незбіг CRC завжди stale,
 * інакше — authored або fresh залежно від маркера авторства.
 * @param {boolean} inSync true — sourceFileCrc перекладу збігається з base
 * @param {boolean} authored true — переклад позначений як authored
 * @returns {string} 'stale' | 'authored' | 'fresh'
 */
function translationState(inSync, authored) {
  if (!inSync) return 'stale'
  return authored ? 'authored' : 'fresh'
}

/**
 * Детермінований стан усіх шарів і перекладів. Без мережі та FS —
 * усі читання через injected `readFile(relPath) → string | null`.
 * @param {object} config результат loadConfig
 * @param {(relPath: string) => string | null} readFile читання файлу полігона
 * @returns {{docs: object[], translations: object[], worst: string, translationsPending: boolean}} повний звіт стану
 */
export function computeStatus(config, readFile) {
  const inspected = new Map()
  /**
   * @param {string} file шлях джерела
   * @returns {{fileCrc: string, essenceCrc: string | null, draft: boolean} | null} стан джерела (кешовано)
   */
  const inspect = file => {
    if (!inspected.has(file)) inspected.set(file, inspectSource(readFile(file)))
    return inspected.get(file)
  }

  const docs = topoOrder(config).map(file => {
    const entry = config.docs[file]
    const isFragment = entry.mode === 'fragment'
    const recorded = recordedSources(readFile(file), isFragment)
    const reasons = []
    if (recorded === null) reasons.push('дока відсутня — потрібен build')

    const sources = entry.sources.map(sourceFile => {
      const current = inspect(sourceFile)
      let state = 'fresh'
      let note = ''
      if (current === null) {
        state = 'stale'
        note = 'джерело відсутнє'
      } else if (current.essenceCrc === null) {
        state = 'no-essence'
        note = 'нема секції «## Суть»'
      } else if (current.draft) {
        state = 'draft'
        note = 'суть — чернетка (essence:draft)'
      } else {
        const rec = recorded?.get(sourceFile)
        if (!rec) {
          state = 'stale'
          note = recorded === null ? '' : 'джерело ще не враховане в доці'
        } else if (rec.essenceCrc !== current.essenceCrc) {
          state = 'stale'
          note = 'суть джерела змінилась'
        } else if (rec.fileCrc !== current.fileCrc) {
          state = 'details-only'
          note = 'змінились лише деталі — перевір «## Суть» джерела і зроби refresh'
        }
      }
      return { file: sourceFile, state, note }
    })

    if (recorded) {
      for (const recordedFile of recorded.keys()) {
        if (!entry.sources.includes(recordedFile)) {
          reasons.push(`у доці записане джерело ${recordedFile}, якого вже нема в конфігу`)
        }
      }
    }

    const state = worstOf([...sources.map(source => source.state), ...(reasons.length ? ['stale'] : [])])
    return { file, layer: entry.layer, state, reasons, sources }
  })

  const translations = []
  for (const file of translationScope(config)) {
    const baseText = readFile(file)
    if (baseText === null) continue // відсутність base уже зарепорчена в шарах
    const baseCrc = fileCrc(parseDoc(baseText).body)
    for (const lang of config.i18n.langs) {
      const translated = readFile(langPath(file, lang))
      let state = 'missing'
      if (translated !== null) {
        const { fm } = parseDoc(translated)
        const authored = fm?.authored === true
        const inSync = fm?.sourceFileCrc === baseCrc
        state = translationState(inSync, authored)
      }
      translations.push({ file: langPath(file, lang), source: file, lang, state })
    }
  }

  return {
    docs,
    translations,
    worst: worstOf(docs.map(doc => doc.state)),
    translationsPending: translations.some(t => t.state === 'missing' || t.state === 'stale')
  }
}

const STATE_ICON = {
  fresh: '✅',
  authored: '✍️',
  'details-only': '🔎',
  draft: '📝',
  stale: '♻️',
  'no-essence': '⛔',
  missing: '∅'
}

/**
 * Людиночитний звіт + exit code.
 * @param {ReturnType<typeof computeStatus>} report результат computeStatus
 * @param {{json?: boolean, strict?: boolean}} [options] опції рендеру
 * @returns {{text: string, exitCode: number}} текст звіту і exit code
 */
export function renderStatus(report, { json = false, strict = false } = {}) {
  const exitCode = statusExitCode(report, strict)
  if (json) return { text: JSON.stringify(report, null, 2), exitCode }

  const lines = ['Шари:']
  for (const doc of report.docs) {
    lines.push(`  ${STATE_ICON[doc.state]} [${doc.layer}] ${doc.file} — ${doc.state}`)
    for (const reason of doc.reasons) lines.push(`      • ${reason}`)
    for (const source of doc.sources) {
      if (source.state === 'fresh') continue
      const noteSuffix = source.note ? ` — ${source.note}` : ''
      lines.push(`      ${STATE_ICON[source.state]} ${source.file}${noteSuffix}`)
    }
  }
  if (report.translations.length) {
    lines.push('Переклади:')
    for (const translation of report.translations) {
      if (translation.state === 'fresh' || translation.state === 'authored') continue
      lines.push(`  ${STATE_ICON[translation.state]} ${translation.file} — ${translation.state}`)
    }
    const pending = report.translations.filter(t => t.state === 'missing' || t.state === 'stale').length
    const done = report.translations.length - pending
    lines.push(`  разом: ${done} свіжих, ${pending} до генерації`)
  }
  lines.push(`Підсумок: ${report.worst}${report.translationsPending ? ' + переклади до генерації' : ''}`)
  return { text: lines.join('\n'), exitCode }
}

/**
 * 0 — усе свіже (details-only толерується без --strict);
 * 1 — є що перебудувати (stale/draft/переклади);
 * 2 — структурна проблема (no-essence).
 * @param {ReturnType<typeof computeStatus>} report результат computeStatus
 * @param {boolean} strict true — details-only теж вважати за розсинхрон
 * @returns {number} exit code 0/1/2
 */
export function statusExitCode(report, strict) {
  const states = new Set(report.docs.flatMap(doc => [doc.state, ...doc.sources.map(s => s.state)]))
  if (states.has('no-essence')) return 2
  if (states.has('stale') || states.has('draft') || report.translationsPending) return 1
  if (strict && states.has('details-only')) return 1
  return 0
}
