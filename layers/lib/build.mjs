/** @see ./docs/build.md */

import { posix } from 'node:path'

import { essenceCrc, fileCrc } from './crc.mjs'
import { layerNumber, topoOrder } from './layers.mjs'
import {
  extractEssence,
  extractFragment,
  formatSourceLine,
  insertEssenceDraft,
  parseDoc,
  parseSourceLine,
  replaceFragment,
  serializeDoc
} from './md.mjs'
import { computeStatus } from './status.mjs'

/**
 * Поточні пари CRC файлу-джерела.
 * @param {string} text вміст файлу
 * @returns {{essenceCrc: string | null, fileCrc: string}} пари CRC (essence — null, якщо секції нема)
 */
export function currentCrcs(text) {
  const { body } = parseDoc(text)
  const essence = extractEssence(body)
  return { essenceCrc: essence ? essenceCrc(essence.text) : null, fileCrc: fileCrc(body) }
}

/**
 * Детермінований refresh: для джерел у стані details-only (суть та сама,
 * деталі змінились) переписує записаний fileCrc — явний акт підтвердження
 * «суть досі валідна». LLM не викликається, вміст док не змінюється.
 * @param {object} config результат loadConfig
 * @param {{readFile: (rel: string) => string | null, writeFile: (rel: string, text: string) => void}} io доступ до файлів полігона
 * @returns {string[]} оновлені доки
 */
export function refreshFileCrcs(config, { readFile, writeFile }) {
  const updated = []
  for (const [file, entry] of Object.entries(config.docs)) {
    const text = readFile(file)
    if (text === null) continue
    const isFragment = entry.mode === 'fragment'
    const rewrite = isFragment ? refreshFragment(text, readFile) : refreshFrontmatter(text, readFile)
    if (rewrite !== null && rewrite !== text) {
      writeFile(file, rewrite)
      updated.push(file)
    }
  }
  return updated
}

/**
 * @param {{file: string, essenceCrc: string, fileCrc: string}} recorded записана пара CRC
 * @param {(rel: string) => string | null} readFile читання файлу полігона
 * @returns {{file: string, essenceCrc: string, fileCrc: string}} освіжена пара (stale лишається як була)
 */
function refreshedSource(recorded, readFile) {
  const text = readFile(recorded.file)
  if (text === null) return recorded
  const current = currentCrcs(text)
  if (current.essenceCrc !== recorded.essenceCrc) return recorded // stale — робота для build
  return { ...recorded, fileCrc: current.fileCrc }
}

/**
 * @param {string} text вміст генерованої доки
 * @param {(rel: string) => string | null} readFile читання файлу полігона
 * @returns {string | null} оновлений текст або null, якщо frontmatter не наш
 */
function refreshFrontmatter(text, readFile) {
  const { fm, body } = parseDoc(text)
  const lines = fm?.layers?.sources
  if (!Array.isArray(lines)) return null
  fm.layers.sources = lines.map(line => formatSourceLine(refreshedSource(parseSourceLine(String(line)), readFile)))
  return serializeDoc(fm, body)
}

/**
 * @param {string} text вміст fragment-хоста
 * @param {(rel: string) => string | null} readFile читання файлу полігона
 * @returns {string | null} оновлений текст або null, якщо маркерів нема
 */
function refreshFragment(text, readFile) {
  const fragment = extractFragment(text)
  if (!fragment) return null
  return replaceFragment(text, {
    layer: fragment.layer,
    sources: fragment.sources.map(source => refreshedSource(source, readFile)),
    content: fragment.inner
  })
}

const H1_RE = /^# (.+)$/m
const ESSENCE_SECTION_RE = /^## Суть\s*$/gm
const HEADING_LINE_RE = /^#{1,6}\s/

/**
 * Локальні моделі часто пишуть заголовок і текст без порожнього рядка між ними
 * (валідне markdown, але не проходить MD022). Вирівнюємо детерміновано, без LLM.
 * @param {string} text вихід моделі
 * @returns {string} той самий текст з порожнім рядком після кожного заголовка
 */
function normalizeHeadingSpacing(text) {
  const lines = text.split('\n')
  const out = []
  for (let i = 0; i < lines.length; i++) {
    out.push(lines[i])
    if (HEADING_LINE_RE.test(lines[i]) && lines[i + 1] !== undefined && lines[i + 1].trim() !== '') {
      out.push('')
    }
  }
  return out.join('\n')
}

/**
 * @param {string} text вміст markdown-файлу
 * @returns {string} текст H1 або порожній рядок
 */
function extractH1(text) {
  const match = text.match(H1_RE)
  return match ? match[1].trim() : ''
}

/**
 * Аудиторія промпту за номером шару: що вищий шар — то простіша мова.
 * @param {number} num номер шару (0 — резюме)
 * @returns {string} інструкція аудиторії
 */
function audienceFor(num) {
  if (num === 0) {
    return (
      'Аудиторія — малий підприємець без технічної освіти. Поясни простими словами, ' +
      'що це перелік задач, які призначаються людям або ШІ, і вони працюють разом. Жодного жаргону.'
    )
  }
  if (num === 1) return 'Аудиторія — технічно грамотний читач, який вирішує, чи заглиблюватись. Мінімум жаргону.'
  return 'Аудиторія — інженер, який хоче зрозуміти підсистему без читання детальних глав.'
}

/**
 * Збирає суті джерел доки; джерела без прийнятої суті блокують генерацію.
 * @param {string[]} sourceFiles відносні шляхи джерел
 * @param {(rel: string) => string | null} readFile читання файлу полігона
 * @returns {{sources: Array<{file: string, essence: string, h1: string, crcs: {essenceCrc: string, fileCrc: string}}>, blockers: string[]}} суті та причини блокування
 */
function collectEssences(sourceFiles, readFile) {
  const sources = []
  const blockers = []
  for (const file of sourceFiles) {
    const text = readFile(file)
    if (text === null) {
      blockers.push(`${file}: джерело відсутнє`)
      continue
    }
    const { body } = parseDoc(text)
    const essence = extractEssence(body)
    if (!essence) {
      blockers.push(`${file}: нема «## Суть» — спершу bootstrap`)
      continue
    }
    if (essence.draft) {
      blockers.push(`${file}: суть — чернетка, зніми маркер essence:draft після рев'ю`)
      continue
    }
    sources.push({ file, essence: essence.text, h1: extractH1(body), crcs: currentCrcs(text) })
  }
  return { sources, blockers }
}

/**
 * Детермінований футер навігації «углиб» — додає builder, не модель.
 * @param {string} file шлях доки, з якої посилаємось
 * @param {Array<{file: string, h1: string}>} sources джерела доки
 * @returns {string} markdown-секція «## Глибше»
 */
function deeperFooter(file, sources) {
  const dir = posix.dirname(file)
  const items = sources.map(source => {
    const rel = posix.relative(dir, source.file)
    return `- [${source.h1 || source.file}](${rel})`
  })
  return `## Глибше\n\n${items.join('\n')}\n`
}

/**
 * Промпт генерації однієї доки шару.
 * @param {object} entry запис конфігу доки
 * @param {Array<{file: string, essence: string, h1: string}>} sources зібрані суті
 * @returns {{system: string, user: string, validate: (content: string) => boolean}} запит для createLlm
 */
function buildRequest(entry, sources) {
  const num = layerNumber(entry.layer)
  const isFragment = entry.mode === 'fragment'
  const system = [
    'Ти — технічний редактор української документації. Ти агрегуєш суті розділів у огляд рівнем вище.',
    audienceFor(num),
    'Пиши українською. Використовуй ЛИШЕ надані суті — нічого не вигадуй і не додавай від себе.',
    isFragment
      ? 'Формат: 2–3 короткі абзаци чистого тексту БЕЗ заголовків, БЕЗ списків посилань.'
      : `Формат: почни з рядка "# ${entry.title}", далі секція "## Суть" (3–6 рядків — що цей огляд експортує нагору), далі розгортка з 2–4 секціями "## …". НЕ додавай секцію "## Глибше" і не встав посилань.`
  ].join(' ')
  const user = sources
    .map((source, index) => `### Джерело ${index + 1}: ${source.h1 || source.file}\n\n${source.essence}`)
    .join('\n\n')
  const validate = isFragment
    ? content => content.length > 0 && !content.includes('#')
    : content =>
        content.startsWith(`# ${entry.title}`) &&
        (content.match(ESSENCE_SECTION_RE) ?? []).length === 1 &&
        !content.includes('## Глибше')
  return { system, user, validate }
}

/**
 * Генерація застарілих док шарів: топологічно знизу вгору, CRC джерел
 * перечитуються перед кожною докою — щойно згенерований L2 одразу дає
 * свіжу суть для L1.
 * @param {object} config результат loadConfig
 * @param {object} deps залежності прогону
 * @param {{readFile: (rel: string) => string | null, writeFile: (rel: string, text: string) => void}} deps.io доступ до файлів
 * @param {{generate: (request: object) => Promise<{content: string, model: string}>}} deps.llm клієнт createLlm
 * @param {boolean} [deps.dryRun] лише показати кандидатів, нічого не писати
 * @param {boolean} [deps.force] перебудувати всі доки незалежно від стану
 * @param {string} [deps.only] обмежитись однією докою
 * @param {(text: string) => void} deps.log прогрес
 * @param {string} deps.today дата YYYY-MM-DD для frontmatter
 * @returns {Promise<{built: string[], skipped: Array<{file: string, reason: string}>, failed: Array<{file: string, reason: string}>}>} підсумок прогону
 */
export async function runBuild(config, { io, llm, dryRun = false, force = false, only, log, today }) {
  const built = []
  const skipped = []
  const failed = []
  const report = computeStatus(config, io.readFile)
  const stateOf = new Map(report.docs.map(doc => [doc.file, doc.state]))

  for (const file of topoOrder(config)) {
    if (only && file !== only) continue
    const entry = config.docs[file]
    // стан перечитується накриво: після запису нижніх шарів верхні все одно stale у звіті
    const needsBuild = force || stateOf.get(file) === 'stale' || io.readFile(file) === null
    if (!needsBuild) {
      skipped.push({ file, reason: `стан ${stateOf.get(file)} — перебудова не потрібна` })
      continue
    }
    const { sources, blockers } = collectEssences(entry.sources, io.readFile)
    if (dryRun) {
      // dry-run не пише нижні шари, тож «джерело відсутнє» для верхніх — очікуване; показуємо всіх кандидатів
      const promptSize = sources.reduce((sum, source) => sum + source.essence.length, 0)
      const note = blockers.length ? ` · блокери зараз: ${blockers.join('; ')}` : ''
      log(`[dry-run] ${file} ← ${entry.sources.join(', ')} (~${promptSize} символів сутей${note})`)
      built.push(file)
      continue
    }
    if (blockers.length) {
      failed.push({ file, reason: blockers.join('; ') })
      continue
    }
    const { system, user, validate } = buildRequest(entry, sources)
    try {
      const { content: rawContent, model } = await llm.generate({ system, user, validate, tier: entry.tier })
      const content = normalizeHeadingSpacing(rawContent)
      io.writeFile(file, composeDoc({ entry, content, model, sources, today, io, file }))
      built.push(file)
      log(`✳️ ${file} ← ${sources.length} джерел (${model})`)
    } catch (error) {
      if (error.code === 'unavailable') throw error
      failed.push({ file, reason: String(error.message ?? error) })
    }
  }
  return { built, skipped, failed }
}

/**
 * Складає фінальний файл доки: frontmatter зі свіжими CRC + тіло LLM +
 * детермінований футер; для fragment — заміна блока в хост-файлі.
 * @param {object} parts складові
 * @param {object} parts.entry запис конфігу доки
 * @param {string} parts.content валідний вихід моделі
 * @param {string} parts.model фактично використана модель
 * @param {Array<{file: string, h1: string, crcs: {essenceCrc: string, fileCrc: string}}>} parts.sources джерела з CRC
 * @param {string} parts.today дата YYYY-MM-DD
 * @param {{readFile: (rel: string) => string | null}} parts.io доступ до файлів
 * @param {string} parts.file шлях доки
 * @returns {string} повний текст файлу для запису
 */
function composeDoc({ entry, content, model, sources, today, io, file }) {
  const sourceLines = sources.map(source => ({ file: source.file, ...source.crcs }))
  if (entry.mode === 'fragment') {
    const host = io.readFile(file)
    if (host === null) throw new Error(`fragment-хост ${file} відсутній — додай маркери layers:${entry.layer}`)
    const deeper = sources
      .map(source => `→ Докладніше: [${source.h1 || source.file}](${posix.relative(posix.dirname(file), source.file)})`)
      .join('\n')
    return replaceFragment(host, {
      layer: entry.layer,
      sources: sourceLines,
      content: `${content}\n\n${deeper}`
    })
  }
  const fm = {
    type: 'layered-doc',
    layer: entry.layer,
    title: entry.title,
    timestamp: today,
    layers: { model, sources: sourceLines.map(line => formatSourceLine(line)) }
  }
  return serializeDoc(fm, `\n${content.trim()}\n\n${deeperFooter(file, sources)}`)
}

/**
 * Bootstrap чернеток «## Суть» для leaf-док, де секції ще нема.
 * Ідемпотентний: доки з суттю пропускаються. Чернетка отримує маркер
 * essence:draft — акцепт (зняття маркера) лишається за людиною.
 * @param {object} config результат loadConfig
 * @param {object} deps залежності прогону
 * @param {{readFile: (rel: string) => string | null, writeFile: (rel: string, text: string) => void}} deps.io доступ до файлів
 * @param {{generate: (request: object) => Promise<{content: string, model: string}>}} deps.llm клієнт createLlm
 * @param {(text: string) => void} deps.log прогрес
 * @returns {Promise<{drafted: string[], skipped: string[], failed: Array<{file: string, reason: string}>}>} підсумок
 */
export async function bootstrapEssences(config, { io, llm, log }) {
  const drafted = []
  const skipped = []
  const failed = []
  for (const file of config.leaves) {
    const text = io.readFile(file)
    if (text === null) {
      failed.push({ file, reason: 'leaf-дока відсутня' })
      continue
    }
    const { fm, body } = parseDoc(text)
    if (extractEssence(body)) {
      skipped.push(file)
      continue
    }
    const system =
      'Ти — технічний редактор української документації. Сформулюй «суть» документа: ' +
      '3–6 рядків про те, що цей документ експортує нагору — його головні рішення і гарантії, ' +
      'без деталей реалізації. Чистий текст без заголовків, списків і посилань. Українською.'
    try {
      const { content } = await llm.generate({
        system,
        user: body,
        validate: draft => draft.length > 0 && !draft.includes('#') && draft.split('\n').length <= 8
      })
      const nextBody = insertEssenceDraft(body, content)
      io.writeFile(file, fm ? serializeDoc(fm, nextBody) : nextBody)
      drafted.push(file)
      log(`📝 ${file} — чернетка суті додана (рев'ю → зніми essence:draft)`)
    } catch (error) {
      if (error.code === 'unavailable') throw error
      failed.push({ file, reason: String(error.message ?? error) })
    }
  }
  return { drafted, skipped, failed }
}
