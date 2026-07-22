/** @see ./docs/layers.md */

import { readFileSync } from 'node:fs'
import { join } from 'node:path'

export const CONFIG_NAME = 'layers.json'

const LAYER_RE = /^L(\d+)$/
const MD_EXT_RE = /\.md$/

/**
 * Читає і валідує конфіг топології `<docsDir>/layers.json`.
 * @param {string} docsDir корінь полігона документації
 * @returns {{docsDir: string, tier: string, maxTokens: number, i18n: {baseLang: string, langs: string[]}, docs: Record<string, {layer: string, title?: string, sources: string[], mode?: string, tier?: string}>, leaves: string[]}} завантажений і валідований конфіг
 */
export function loadConfig(docsDir) {
  let raw
  try {
    raw = readFileSync(join(docsDir, CONFIG_NAME), 'utf8')
  } catch {
    throw new Error(`Не знайдено ${join(docsDir, CONFIG_NAME)} — це не тека шарової документації`)
  }
  const parsed = JSON.parse(raw)
  const config = {
    docsDir,
    tier: parsed.tier ?? 'avg',
    maxTokens: parsed.maxTokens ?? 4096,
    i18n: { baseLang: parsed.i18n?.baseLang ?? 'uk', langs: parsed.i18n?.langs ?? [] },
    docs: parsed.docs ?? {},
    leaves: []
  }
  config.leaves = deriveLeaves(config.docs)
  const errors = validateConfig(config)
  if (errors.length) throw new Error(`Невалідний ${CONFIG_NAME}:\n- ${errors.join('\n- ')}`)
  return config
}

/**
 * Leaf — файл, що фігурує в sources, але не має власного запису в docs.
 * @param {Record<string, {sources: string[]}>} docs записи конфігу
 * @returns {string[]} відсортовані шляхи leaf-файлів
 */
function deriveLeaves(docs) {
  const generated = new Set(Object.keys(docs))
  const leaves = new Set()
  for (const entry of Object.values(docs)) {
    for (const source of entry.sources ?? []) {
      if (!generated.has(source)) leaves.add(source)
    }
  }
  return [...leaves].toSorted()
}

/**
 * Номер шару з мітки `L<n>`; менший n — вищий (агрегованіший) шар.
 * @param {string} layer мітка шару (напр. `L2`)
 * @returns {number} номер шару або NaN, якщо мітка невалідна
 */
export function layerNumber(layer) {
  const match = LAYER_RE.exec(layer ?? '')
  return match ? Number(match[1]) : NaN
}

/**
 * Перевіряє мітку шару, непорожність sources і невідомий mode одного запису.
 * @param {string} file шлях доки
 * @param {{layer: string, sources?: string[], mode?: string}} entry запис конфігу
 * @param {number} num номер шару доки (може бути NaN)
 * @returns {string[]} помилки форми запису
 */
function validateEntryShape(file, entry, num) {
  const errors = []
  if (Number.isNaN(num)) errors.push(`${file}: невалідна мітка шару '${entry.layer}' (очікую L<n>)`)
  if (!entry.sources?.length) errors.push(`${file}: порожній список sources`)
  if (entry.mode && entry.mode !== 'fragment') errors.push(`${file}: невідомий mode '${entry.mode}'`)
  return errors
}

/**
 * Перевіряє напрямок і самопосилання джерел одного запису; заповнює sourcedBy.
 * @param {string} file шлях доки
 * @param {{layer: string, sources?: string[]}} entry запис конфігу
 * @param {number} num номер шару доки
 * @param {Record<string, {layer: string, sources?: string[]}>} docs усі доки конфігу
 * @param {Map<string, string>} sourcedBy мапа джерело → дока, що його використовує (мутується)
 * @returns {string[]} помилки джерел запису
 */
function validateSources(file, entry, num, docs, sourcedBy) {
  const errors = []
  for (const source of entry.sources ?? []) {
    sourcedBy.set(source, file)
    if (source === file) errors.push(`${file}: посилається сам на себе`)
    const sourceEntry = docs[source]
    if (sourceEntry && layerNumber(sourceEntry.layer) <= num) {
      errors.push(`${file} (${entry.layer}): джерело ${source} (${sourceEntry.layer}) не з нижчого шару`)
    }
  }
  return errors
}

/**
 * Перевіряє, що жодна fragment-дока не фігурує як джерело іншої.
 * @param {Record<string, {mode?: string}>} docs усі доки конфігу
 * @param {Map<string, string>} sourcedBy мапа джерело → дока, що його використовує
 * @returns {string[]} помилки fragment-обмеження
 */
function validateFragmentSources(docs, sourcedBy) {
  const errors = []
  for (const [file, entry] of Object.entries(docs)) {
    if (entry.mode === 'fragment' && sourcedBy.has(file)) {
      errors.push(`${file}: fragment-дока не може бути джерелом для ${sourcedBy.get(file)}`)
    }
  }
  return errors
}

/**
 * Перевіряє топологію: мітки шарів, напрямок джерел (лише знизу вгору),
 * fragment-обмеження. Порядок «джерело — строго нижчий шар» унеможливлює цикли.
 * @param {ReturnType<typeof loadConfig>} config результат loadConfig
 * @returns {string[]} помилки (порожньо — конфіг валідний)
 */
export function validateConfig(config) {
  const errors = []
  const sourcedBy = new Map()
  for (const [file, entry] of Object.entries(config.docs)) {
    const num = layerNumber(entry.layer)
    errors.push(...validateEntryShape(file, entry, num), ...validateSources(file, entry, num, config.docs, sourcedBy))
  }
  errors.push(...validateFragmentSources(config.docs, sourcedBy))
  return errors
}

/**
 * Порядок перебудови: знизу вгору (більший номер шару — раніше);
 * всередині шару — порядок оголошення в конфігу.
 * @param {ReturnType<typeof loadConfig>} config результат loadConfig
 * @returns {string[]} шляхи док у порядку перебудови
 */
export function topoOrder(config) {
  return Object.keys(config.docs).toSorted(
    (a, b) => layerNumber(config.docs[b].layer) - layerNumber(config.docs[a].layer)
  )
}

/**
 * Файли, що підлягають перекладу: leaf-доки + всі доки конфігу.
 * @param {ReturnType<typeof loadConfig>} config результат loadConfig
 * @returns {string[]} шляхи файлів у scope перекладу
 */
export function translationScope(config) {
  return [...config.leaves, ...Object.keys(config.docs)]
}

/**
 * Шлях derived-перекладу: `architecture/graph.md` + `en` → `architecture/graph.en.md`.
 * @param {string} file шлях base-файлу
 * @param {string} lang код мови
 * @returns {string} шлях мовної версії файлу
 */
export function langPath(file, lang) {
  return file.replace(MD_EXT_RE, () => `.${lang}.md`)
}
