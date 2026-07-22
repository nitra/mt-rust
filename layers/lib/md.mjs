/** @see ./docs/md.md */

export const DRAFT_MARKER = '<!-- essence:draft -->'
const ESSENCE_HEADING = /^##\s+Суть\s*$/
const NEEDS_QUOTE = /[:#'",[\]{}|>&*!%@`]|^\s|\s$|^$/
const KEY_RE = /^[A-Za-z_][\w.-]*$/
const SPLIT_WS_RE = /\s+/
const FRAGMENT_OPEN = /<!-- layers:(\S+) sources: (.*?) -->/
const FRAGMENT_CLOSE = /<!-- \/layers:(\S+) -->/
/** Ключі, чиї рядкові значення завжди лаповані — так з ними працює вся наявна документація репо. */
const ALWAYS_QUOTE_KEYS = new Set(['description'])
/** Немаркований (non-enumerable) прапор на масиві: розібраний як inline `[a, b]`, а не block-list. */
const FLOW_FLAG = Symbol('flow')

/**
 * Розбирає документ на frontmatter (YAML-сабсет) і тіло.
 * Тіло повертається байт-у-байт (усе після закривального `---`).
 * @param {string} text повний вміст файлу
 * @returns {{fm: Record<string, unknown> | null, body: string}} frontmatter (null — його нема) і тіло
 */
export function parseDoc(text) {
  if (!text.startsWith('---\n')) return { fm: null, body: text }
  const end = text.indexOf('\n---\n', 3)
  if (end === -1) return { fm: null, body: text }
  const fmRaw = text.slice(4, end + 1)
  return { fm: parseFrontmatter(fmRaw), body: text.slice(end + 5) }
}

/**
 * Серіалізує frontmatter + тіло назад у документ.
 * Для власних док рушія round-trip `parseDoc(serializeDoc(fm, body))` байт-стабільний.
 * @param {Record<string, unknown>} fm розібраний frontmatter
 * @param {string} body тіло документа
 * @returns {string} повний текст файлу
 */
export function serializeDoc(fm, body) {
  return `---\n${serializeFrontmatter(fm)}---\n${body}`
}

/**
 * @param {string} fmRaw сирий текст frontmatter (без роздільників `---`)
 * @returns {Record<string, unknown>} розібрана мапа верхнього рівня
 */
function parseFrontmatter(fmRaw) {
  const [obj] = parseMap(fmRaw.split('\n'), 0, 0)
  return obj
}

/**
 * Розбирає рядок `key: value` (або просто `key:`). Ключ — суворо ідентифікатор,
 * значення після колону — усе до кінця рядка (без further-нідедлайн ескейпів).
 * @param {string} line один trimmed рядок frontmatter
 * @returns {{key: string, rawValue: string | undefined} | null} null — рядок не «key: value»
 */
function parseKeyValue(line) {
  const colon = line.indexOf(':')
  if (colon === -1) return null
  const key = line.slice(0, colon)
  if (!KEY_RE.test(key)) return null
  const rest = line.slice(colon + 1).trim()
  return { key, rawValue: rest === '' ? undefined : rest }
}

/**
 * Розбирає block-list (`- item` рядки) від `start`, поки відступ не менший за `indent`.
 * @param {string[]} lines усі рядки frontmatter
 * @param {number} start індекс першого рядка списку
 * @param {number} indent мінімальний відступ елементів списку
 * @returns {[unknown[], number]} елементи і індекс першого рядка після списку
 */
function parseBlockList(lines, start, indent) {
  const items = []
  let i = start
  while (i < lines.length) {
    const itemRaw = lines[i]
    if (!itemRaw.trim()) {
      i++
      continue
    }
    if (!itemRaw.trim().startsWith('- ') || itemRaw.length - itemRaw.trimStart().length <= indent) break
    items.push(parseScalar(itemRaw.trim().slice(2)))
    i++
  }
  return [items, i]
}

/**
 * Розбирає ключ без inline-значення: наступний глибший рядок вирішує — block-list чи вкладена мапа.
 * @param {string[]} lines усі рядки frontmatter
 * @param {number} i індекс першого рядка після `key:`
 * @param {number} indent відступ поточного рівня
 * @returns {[unknown, number]} значення (масив/мапа/null) і індекс наступного нерозібраного рядка
 */
function parseNestedValue(lines, i, indent) {
  const next = lines.slice(i).find(line => line.trim())
  if (next?.trim().startsWith('- ')) return parseBlockList(lines, i, indent)
  if (next && next.length - next.trimStart().length > indent) return parseMap(lines, i, indent + 2)
  return [null, i]
}

/**
 * Рекурсивний розбір мапи з фіксованим відступом; підтримувана глибина —
 * рівно та, яку пише сам рушій (вкладена мапа + списки скалярів).
 * @param {string[]} lines усі рядки frontmatter
 * @param {number} start індекс першого рядка цього рівня
 * @param {number} indent мінімальний відступ рядків цього рівня
 * @returns {[Record<string, unknown>, number]} розібрана мапа і індекс першого рядка за нею
 */
function parseMap(lines, start, indent) {
  /** @type {Record<string, unknown>} */
  const obj = {}
  let i = start
  while (i < lines.length) {
    const raw = lines[i]
    if (!raw.trim()) {
      i++
      continue
    }
    const lineIndent = raw.length - raw.trimStart().length
    if (lineIndent < indent) break
    const parsed = parseKeyValue(raw.trim())
    if (!parsed) break
    i++
    if (parsed.rawValue !== undefined) {
      obj[parsed.key] = parseScalar(parsed.rawValue)
      continue
    }
    const [value, nextIndex] = parseNestedValue(lines, i, indent)
    obj[parsed.key] = value
    i = nextIndex
  }
  return [obj, i]
}

/**
 * Ділить вміст inline-масиву на елементи за комами верхнього рівня
 * (кома всередині `'...'` не рахується роздільником).
 * @param {string} inner текст між `[` і `]`
 * @returns {string[]} нерозібрані шматки елементів
 */
function splitFlowItems(inner) {
  const parts = []
  let current = ''
  let inQuote = false
  for (const ch of inner) {
    if (ch === "'") inQuote = !inQuote
    if (ch === ',' && !inQuote) {
      parts.push(current)
      current = ''
    } else {
      current += ch
    }
  }
  parts.push(current)
  return parts
}

/**
 * Розбирає inline-масив `[a, 'b, c', true]` у справжній масив, поміченний FLOW_FLAG
 * для збереження inline-формату при серіалізації.
 * @param {string} value повний текст значення разом із дужками
 * @returns {unknown[]} розібраний масив
 */
function parseFlowArray(value) {
  const inner = value.slice(1, -1).trim()
  const items = inner === '' ? [] : splitFlowItems(inner).map(item => parseScalar(item.trim()))
  Object.defineProperty(items, FLOW_FLAG, { value: true, enumerable: false })
  return items
}

/**
 * @param {string} raw сирий текст значення (усе після `key:`)
 * @returns {unknown} розібране значення: boolean, масив або рядок
 */
function parseScalar(raw) {
  const value = raw.trim()
  if (value === 'true') return true
  if (value === 'false') return false
  if (value.startsWith('[') && value.endsWith(']')) return parseFlowArray(value)
  if (value.startsWith("'") && value.endsWith("'") && value.length >= 2) {
    return value.slice(1, -1).replaceAll("''", "'")
  }
  return value
}

/**
 * @param {Record<string, unknown>} obj мапа для серіалізації
 * @param {number} [indent] поточний відступ (рекурсія для вкладених мап)
 * @returns {string} YAML-сабсет текст (кожен рядок з `\n`)
 */
function serializeFrontmatter(obj, indent = 0) {
  const pad = ' '.repeat(indent)
  let out = ''
  for (const [key, value] of Object.entries(obj)) {
    if (value === null || value === undefined) {
      out += `${pad}${key}:\n`
    } else if (Array.isArray(value)) {
      out += value[FLOW_FLAG] ? serializeFlowArray(pad, key, value) : serializeBlockList(pad, key, value)
    } else if (typeof value === 'object') {
      out += `${pad}${key}:\n${serializeFrontmatter(value, indent + 2)}`
    } else {
      const rendered =
        ALWAYS_QUOTE_KEYS.has(key) && typeof value === 'string' ? quoteScalar(value) : serializeScalar(value)
      out += `${pad}${key}: ${rendered}\n`
    }
  }
  return out
}

/**
 * @param {string} pad відступ поточного рівня
 * @param {string} key ключ масиву
 * @param {unknown[]} items елементи масиву
 * @returns {string} рядок `key: [a, b]\n`
 */
function serializeFlowArray(pad, key, items) {
  return `${pad}${key}: [${items.map(item => serializeScalar(item)).join(', ')}]\n`
}

/**
 * @param {string} pad відступ поточного рівня
 * @param {string} key ключ масиву
 * @param {unknown[]} items елементи масиву
 * @returns {string} block-list `key:\n  - a\n  - b\n`
 */
function serializeBlockList(pad, key, items) {
  let out = `${pad}${key}:\n`
  for (const item of items) out += `${pad}  - ${serializeScalar(item)}\n`
  return out
}

/**
 * @param {unknown} value скалярне значення
 * @returns {string} YAML-представлення (лаповане, якщо містить спецсимволи)
 */
function serializeScalar(value) {
  if (typeof value === 'boolean') return String(value)
  const text = String(value)
  return NEEDS_QUOTE.test(text) ? quoteScalar(text) : text
}

/**
 * @param {string} text значення без лапок
 * @returns {string} завжди лаповане YAML-значення
 */
function quoteScalar(text) {
  return `'${text.replaceAll("'", "''")}'`
}

/**
 * Витягає блок «## Суть» із тіла документа.
 * @param {string} body тіло документа (без frontmatter)
 * @returns {{text: string, draft: boolean} | null} null — секції нема
 */
export function extractEssence(body) {
  const lines = body.split('\n')
  const start = lines.findIndex(line => ESSENCE_HEADING.test(line))
  if (start === -1) return null
  let end = lines.length
  for (let i = start + 1; i < lines.length; i++) {
    if (lines[i].startsWith('## ')) {
      end = i
      break
    }
  }
  const sectionLines = lines.slice(start + 1, end)
  const draft = sectionLines.some(line => line.trim() === DRAFT_MARKER)
  const text = sectionLines
    .filter(line => line.trim() !== DRAFT_MARKER)
    .join('\n')
    .trim()
  return { text, draft }
}

/**
 * Вставляє секцію «## Суть» (чернетку) після H1 і вступного blockquote.
 * @param {string} body тіло документа
 * @param {string} essenceText текст чернетки суті
 * @returns {string} тіло з вставленою секцією
 */
export function insertEssenceDraft(body, essenceText) {
  const lines = body.split('\n')
  let insertAt = 0
  const h1 = lines.findIndex(line => line.startsWith('# '))
  if (h1 !== -1) {
    insertAt = h1 + 1
    while (insertAt < lines.length && (lines[insertAt].startsWith('>') || !lines[insertAt].trim())) {
      insertAt++
    }
  }
  const section = ['## Суть', '', DRAFT_MARKER, essenceText.trim(), '']
  return [...lines.slice(0, insertAt), ...section, ...lines.slice(insertAt)].join('\n')
}

/**
 * @param {string} line рядок виду 'шлях essenceCrc fileCrc'
 * @returns {{file: string, essenceCrc: string, fileCrc: string}} розібрана трійка
 */
export function parseSourceLine(line) {
  const parts = line.trim().split(SPLIT_WS_RE)
  if (parts.length !== 3) throw new Error(`Невалідний source-рядок: '${line}'`)
  const [file, essence, body] = parts
  return { file, essenceCrc: essence, fileCrc: body }
}

/**
 * @param {{file: string, essenceCrc: string, fileCrc: string}} source трійка джерела
 * @returns {string} рядок 'шлях essenceCrc fileCrc'
 */
export function formatSourceLine({ file, essenceCrc, fileCrc }) {
  return `${file} ${essenceCrc} ${fileCrc}`
}

/**
 * Знаходить fragment-блок рушія у тексті (напр. L0 в index.md).
 * @param {string} text повний вміст хост-файлу
 * @returns {{layer: string, sources: Array<{file: string, essenceCrc: string, fileCrc: string}>, inner: string} | null} null — маркерів нема
 */
export function extractFragment(text) {
  const open = text.match(FRAGMENT_OPEN)
  if (!open || open.index === undefined) return null
  const close = text.slice(open.index).match(FRAGMENT_CLOSE)
  if (!close || close.index === undefined) return null
  const innerStart = open.index + open[0].length
  const innerEnd = open.index + close.index
  let inner = text.slice(innerStart, innerEnd)
  if (inner.startsWith('\n')) inner = inner.slice(1)
  if (inner.endsWith('\n')) inner = inner.slice(0, -1)
  return {
    layer: open[1],
    sources: open[2].split(',').map(part => parseSourceLine(part)),
    inner
  }
}

/**
 * Замінює fragment-блок: оновлює sources у маркері та вміст між маркерами.
 * @param {string} text повний вміст хост-файлу
 * @param {{layer: string, sources: Array<{file: string, essenceCrc: string, fileCrc: string}>, content: string}} fragment нові дані блоку
 * @returns {string} текст з оновленим блоком; решта файлу недоторкана
 */
export function replaceFragment(text, { layer, sources, content }) {
  const open = text.match(FRAGMENT_OPEN)
  if (!open || open.index === undefined) throw new Error(`Fragment-маркер layers:${layer} не знайдено`)
  const close = text.slice(open.index).match(FRAGMENT_CLOSE)
  if (!close || close.index === undefined) throw new Error(`Закривальний маркер /layers:${layer} не знайдено`)
  const sourceList = sources.map(source => formatSourceLine(source)).join(', ')
  const block = `<!-- layers:${layer} sources: ${sourceList} -->\n${content.trim()}\n<!-- /layers:${layer} -->`
  return text.slice(0, open.index) + block + text.slice(open.index + close.index + close[0].length)
}
