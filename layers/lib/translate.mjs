/** @see ./docs/translate.md */

import { posix } from 'node:path'

import { fileCrc } from './crc.mjs'
import { langPath, translationScope } from './layers.mjs'
import { parseDoc, serializeDoc } from './md.mjs'

/** Рядки-маркери fragment-блоків рушія — у переклад не потрапляють. */
const MARKER_LINE_RE = /^<!-- \/?layers:.*-->\s*$/
const FENCE_RE = /^```/gm
const LINK_RE = /\]\(([^)\s]+)\)/g
/** Понад цей обсяг (символів) тіло перекладається почастинно за H2-секціями. */
const CHUNK_LIMIT = 12000

/**
 * Прибирає рядки fragment-маркерів, лишаючи їхній вміст.
 * @param {string} body тіло base-документа
 * @returns {string} тіло без службових маркерів
 */
function stripLayerMarkers(body) {
  return body
    .split('\n')
    .filter(line => !MARKER_LINE_RE.test(line))
    .join('\n')
}

/**
 * Мультимножина цілей посилань — для перевірки, що переклад їх не загубив.
 * @param {string} text markdown
 * @returns {string} канонічний підпис цілей
 */
function linkSignature(text) {
  return Array.from(text.matchAll(LINK_RE), match => match[1])
    .toSorted()
    .join('\n')
}

/**
 * Структурна перевірка перекладу відносно оригіналу: кількість fence-рядків
 * і цілі посилань мають збігтися (тексти лінків перекладаються, цілі — ні).
 * @param {string} original фрагмент оригіналу
 * @param {string} translated фрагмент перекладу
 * @returns {boolean} true — структура збережена
 */
function structurePreserved(original, translated) {
  const fencesEqual = (original.match(FENCE_RE) ?? []).length === (translated.match(FENCE_RE) ?? []).length
  return fencesEqual && linkSignature(original) === linkSignature(translated)
}

/**
 * Ділить тіло на фрагменти по межах H2-секцій, кожен ≤ CHUNK_LIMIT
 * (наскільки дозволяють секції); детермінована збірка — простий join.
 * @param {string} body тіло документа
 * @returns {string[]} фрагменти у вихідному порядку
 */
export function splitBySections(body) {
  if (body.length <= CHUNK_LIMIT) return [body]
  const starts = [0]
  const lines = body.split('\n')
  let offset = 0
  for (const line of lines) {
    if (line.startsWith('## ') && offset > 0) starts.push(offset)
    offset += line.length + 1
  }
  const blocks = starts.map((start, index) => body.slice(start, starts[index + 1]))
  const chunks = []
  let current = ''
  for (const block of blocks) {
    if (current && current.length + block.length > CHUNK_LIMIT) {
      chunks.push(current)
      current = ''
    }
    current += block
  }
  if (current) chunks.push(current)
  return chunks
}

/**
 * Переписує відносні `.md`-посилання на мовні файли, якщо ціль — у scope перекладу.
 * @param {string} text перекладений markdown
 * @param {string} file шлях base-доки (для резолву відносних цілей)
 * @param {string} lang код мови
 * @param {Set<string>} scope файли, що мають переклади
 * @returns {string} текст із мовними посиланнями
 */
export function rewriteLinks(text, file, lang, scope) {
  const dir = posix.dirname(file)
  return text.replaceAll(LINK_RE, (whole, target) => {
    const [path, anchor] = String(target).split('#')
    if (!path?.endsWith('.md') || path.includes('://')) return whole
    const resolved = posix.normalize(posix.join(dir, path))
    if (!scope.has(resolved)) return whole
    const suffix = anchor ? `#${anchor}` : ''
    return `](${langPath(path, lang)}${suffix})`
  })
}

/**
 * Derived-переклади всіх док scope: staleness за fileCrc base-версії
 * (переклад віддзеркалює деталі, не лише суть). Authored-переклад
 * не перезаписується, поки base не змінився (i18n.md).
 * @param {object} config результат loadConfig
 * @param {object} deps залежності прогону
 * @param {{readFile: (rel: string) => string | null, writeFile: (rel: string, text: string) => void}} deps.io доступ до файлів
 * @param {{generate: (request: object) => Promise<{content: string, model: string}>}} deps.llm клієнт createLlm
 * @param {string} [deps.lang] обмежитись однією мовою
 * @param {string} [deps.only] обмежитись однією докою
 * @param {boolean} [deps.dryRun] лише показати кандидатів
 * @param {(text: string) => void} deps.log прогрес
 * @param {string} deps.today дата YYYY-MM-DD
 * @returns {Promise<{written: string[], skipped: string[], failed: Array<{file: string, reason: string}>}>} підсумок
 */
export async function runTranslate(config, deps) {
  const summary = { written: [], skipped: [], failed: [] }
  const langs = deps.lang ? [deps.lang] : config.i18n.langs
  const scope = new Set(translationScope(config))

  for (const file of translationScope(config)) {
    if (deps.only && file !== deps.only) continue
    const base = deps.io.readFile(file)
    if (base === null) continue
    const { body } = parseDoc(base)
    for (const code of langs) {
      await translateTarget({ ...deps, file, body, baseCrc: fileCrc(body), code, scope, summary })
    }
  }
  return summary
}

/**
 * Обробляє одну пару «дока × мова»: skip свіжого, dry-run, генерація й запис.
 * @param {object} task контекст пари
 * @param {{readFile: (rel: string) => string | null, writeFile: (rel: string, text: string) => void}} task.io доступ до файлів
 * @param {{generate: (request: object) => Promise<{content: string, model: string}>}} task.llm клієнт createLlm
 * @param {string} task.file шлях base-доки
 * @param {string} task.body тіло base-доки
 * @param {string} task.baseCrc fileCrc base-тіла
 * @param {string} task.code код мови
 * @param {Set<string>} task.scope файли scope перекладу
 * @param {{written: string[], skipped: string[], failed: Array<{file: string, reason: string}>}} task.summary акумулятор підсумку
 * @param {boolean} [task.dryRun] лише показати кандидатів
 * @param {(text: string) => void} task.log прогрес
 * @param {string} task.today дата YYYY-MM-DD
 * @returns {Promise<void>} результат — у task.summary
 */
async function translateTarget({ io, llm, file, body, baseCrc, code, scope, summary, dryRun = false, log, today }) {
  const target = langPath(file, code)
  const existing = io.readFile(target)
  if (existing !== null) {
    const { fm } = parseDoc(existing)
    if (fm?.sourceFileCrc === baseCrc) {
      summary.skipped.push(target)
      return
    }
    if (fm?.authored === true) log(`⚠️ ${target}: authored-переклад застарів — перегенеровую (base змінився)`)
  }
  if (dryRun) {
    log(`[dry-run] ${target} ← ${file}`)
    summary.written.push(target)
    return
  }
  try {
    const { content, model } = await translateBody(stripLayerMarkers(body), code, llm)
    const fm = {
      type: 'layered-translation',
      source: file,
      lang: code,
      sourceFileCrc: baseCrc,
      authored: false,
      translated: today,
      model
    }
    io.writeFile(target, serializeDoc(fm, `\n${rewriteLinks(content, file, code, scope).trim()}\n`))
    summary.written.push(target)
    log(`🌐 ${target} (${model})`)
  } catch (error) {
    if (error.code === 'unavailable') throw error
    summary.failed.push({ file: target, reason: String(error.message ?? error) })
  }
}

/**
 * Contract-aware переклад тіла: почастинно за секціями, зі структурною валідацією.
 * @param {string} body тіло base-доки без маркерів
 * @param {string} lang код мови BCP-47
 * @param {{generate: (request: object) => Promise<{content: string, model: string}>}} llm клієнт createLlm
 * @returns {Promise<{content: string, model: string}>} перекладене тіло і фактична модель
 */
async function translateBody(body, lang, llm) {
  const system = [
    `Ти — технічний перекладач. Переклади markdown мовою з кодом BCP-47 '${lang}'.`,
    'Збережи структуру: заголовки тих самих рівнів, списки, таблиці, blockquote.',
    'НЕ перекладай і не змінюй: вміст code fences, inline-код у зворотних лапках,',
    'шляхи файлів, URL, цілі посилань у дужках (), frontmatter-ключі, ідентифікатори.',
    'Поверни ЛИШЕ перекладений markdown без пояснень і обгорток.'
  ].join(' ')
  const parts = []
  let model = ''
  for (const chunk of splitBySections(body)) {
    if (!chunk.trim()) continue
    const result = await llm.generate({
      system,
      user: chunk,
      validate: translated => structurePreserved(chunk, translated)
    })
    parts.push(result.content.trim())
    model = result.model
  }
  return { content: parts.join('\n\n'), model }
}
