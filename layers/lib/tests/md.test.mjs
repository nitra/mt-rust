import { describe, expect, test } from 'vitest'

import {
  DRAFT_MARKER,
  extractEssence,
  extractFragment,
  formatSourceLine,
  insertEssenceDraft,
  parseDoc,
  parseSourceLine,
  replaceFragment,
  serializeDoc
} from '../md.mjs'

const INVALID_SOURCE_LINE_RE = /Невалідний/
const MARKER_NOT_FOUND_RE = /не знайдено/

const GENERATED_DOC = `---
type: layered-doc
layer: L2
title: 'Ядро: граф задач і координація через git'
description: 'Агрегований огляд графа і git-координації'
timestamp: 2026-07-12
layers:
  model: omlx/gemma-4-e2b-it-4bit
  sources:
    - architecture/overview.md e58c1f00 9f8e7d6c
    - architecture/graph.md 41ab02de 77c3a1b9
---

# Ядро

## Суть

Ядро — граф задач у git.

## Розгортка

Деталі.
`

describe('parseDoc / serializeDoc', () => {
  test('round-trip власної доки байт-у-байт', () => {
    const { fm, body } = parseDoc(GENERATED_DOC)
    expect(fm).not.toBeNull()
    expect(serializeDoc(fm, body)).toBe(GENERATED_DOC)
  })

  test('розбирає вкладену мапу, список і лаповані значення', () => {
    const { fm } = parseDoc(GENERATED_DOC)
    expect(fm.layer).toBe('L2')
    expect(fm.title).toBe('Ядро: граф задач і координація через git')
    expect(fm.layers.model).toBe('omlx/gemma-4-e2b-it-4bit')
    expect(fm.layers.sources).toEqual([
      'architecture/overview.md e58c1f00 9f8e7d6c',
      'architecture/graph.md 41ab02de 77c3a1b9'
    ])
  })

  test('булеві значення парсяться і серіалізуються', () => {
    const doc = serializeDoc({ authored: false, stale: true }, '\nтіло\n')
    const { fm } = parseDoc(doc)
    expect(fm.authored).toBe(false)
    expect(fm.stale).toBe(true)
  })

  test('документ без frontmatter → fm: null, тіло без змін', () => {
    const text = '# Просто markdown\n'
    expect(parseDoc(text)).toEqual({ fm: null, body: text })
  })

  test('незакритий frontmatter трактується як тіло', () => {
    const text = '---\ntype: x\n# без закриття\n'
    expect(parseDoc(text).fm).toBeNull()
  })

  test("апостроф в лапованому значенні екранується '' і відновлюється", () => {
    const doc = serializeDoc({ title: "обʼєднання 'графа'" }, '\n')
    expect(parseDoc(doc).fm.title).toBe("обʼєднання 'графа'")
  })

  test('description завжди лаповане, навіть без спецсимволів (конвенція репо)', () => {
    const doc = serializeDoc({ description: 'Просте речення без ком і двокрапок' }, '\n')
    expect(doc).toContain("description: 'Просте речення без ком і двокрапок'\n")
  })
})

const ARCHITECTURE_DOC = `---
type: architecture
description: 'Акаунти і ключі пристроїв, relay та membership, ролі, три approval-гейти з Ed25519-підписами, push'
tags: [access, relay, membership, approvals, security]
timestamp: 2026-07-07
---

# Люди, пристрої, доступ

Текст.
`

describe('parseDoc / serializeDoc: чужі доки з inline-масивами (формат B)', () => {
  test('round-trip авторської доки з `tags: [...]` байт-у-байт', () => {
    const { fm, body } = parseDoc(ARCHITECTURE_DOC)
    expect(fm.tags).toEqual(['access', 'relay', 'membership', 'approvals', 'security'])
    expect(serializeDoc(fm, body)).toBe(ARCHITECTURE_DOC)
  })

  test('inline-масив лишається справжнім масивом, не рядком', () => {
    const { fm } = parseDoc(ARCHITECTURE_DOC)
    expect(Array.isArray(fm.tags)).toBe(true)
    expect(fm.tags).toHaveLength(5)
  })

  test('block-list (наш формат) і inline-масив (чужий формат) не плутаються', () => {
    const { fm: generatedFm } = parseDoc(GENERATED_DOC)
    expect(generatedFm.layers.sources).toEqual([
      'architecture/overview.md e58c1f00 9f8e7d6c',
      'architecture/graph.md 41ab02de 77c3a1b9'
    ])
    // block-list парситься без FLOW-прапора → серіалізується назад у block-list, не inline
    const reserialized = serializeDoc(generatedFm, parseDoc(GENERATED_DOC).body)
    expect(reserialized).toContain('    - architecture/overview.md e58c1f00 9f8e7d6c')
    expect(reserialized).not.toContain('sources: [')
  })

  test('масив, побудований наново (без флагу), серіалізується як block-list', () => {
    const doc = serializeDoc({ tags: ['a', 'b'] }, '\n')
    expect(doc).toContain('tags:\n  - a\n  - b\n')
  })

  test('порожній inline-масив round-trip', () => {
    const doc = '---\ntags: []\n---\n\nтіло\n'
    const { fm, body } = parseDoc(doc)
    expect(fm.tags).toEqual([])
    expect(serializeDoc(fm, body)).toBe(doc)
  })
})

describe('extractEssence', () => {
  test('витягає текст суті без заголовка', () => {
    const essence = extractEssence('# Док\n\n## Суть\n\nПерший рядок.\nДругий.\n\n## Далі\n\nІнше.\n')
    expect(essence).toEqual({ text: 'Перший рядок.\nДругий.', draft: false })
  })

  test('нема секції → null', () => {
    expect(extractEssence('# Док\n\n## Огляд\n')).toBeNull()
  })

  test('draft-маркер розпізнається і вилучається з тексту', () => {
    const essence = extractEssence(`# Док\n\n## Суть\n\n${DRAFT_MARKER}\nЧернетка суті.\n`)
    expect(essence).toEqual({ text: 'Чернетка суті.', draft: true })
  })

  test('секція в кінці файлу (без наступного ##)', () => {
    expect(extractEssence('# Док\n\n## Суть\n\nОстання.\n').text).toBe('Остання.')
  })
})

describe('insertEssenceDraft', () => {
  test('вставляє після H1 і вступного blockquote', () => {
    const body = '# Заголовок\n\n> Вступ.\n\n## Перша секція\n\nТекст.\n'
    const result = insertEssenceDraft(body, 'Суть доки.')
    expect(result.indexOf('## Суть')).toBeGreaterThan(result.indexOf('> Вступ.'))
    expect(result.indexOf('## Суть')).toBeLessThan(result.indexOf('## Перша секція'))
    const essence = extractEssence(result)
    expect(essence).toEqual({ text: 'Суть доки.', draft: true })
  })

  test('без H1 — вставка на початок', () => {
    const result = insertEssenceDraft('Просто текст.\n', 'Суть.')
    expect(result.startsWith('## Суть')).toBe(true)
  })
})

describe('source-рядки', () => {
  test('parse/format round-trip', () => {
    const line = 'architecture/graph.md 41ab02de 77c3a1b9'
    expect(formatSourceLine(parseSourceLine(line))).toBe(line)
  })

  test('невалідний рядок кидає помилку', () => {
    expect(() => parseSourceLine('лише-шлях')).toThrow(INVALID_SOURCE_LINE_RE)
  })
})

describe('fragment-блоки', () => {
  const TEXT = [
    '# Індекс',
    '',
    '<!-- layers:L0 sources: overview/index.md e58c1f00 9f8e7d6c -->',
    'Старе резюме.',
    '<!-- /layers:L0 -->',
    '',
    '## Решта',
    ''
  ].join('\n')

  test('extractFragment читає шар, джерела і вміст', () => {
    const fragment = extractFragment(TEXT)
    expect(fragment.layer).toBe('L0')
    expect(fragment.sources).toEqual([{ file: 'overview/index.md', essenceCrc: 'e58c1f00', fileCrc: '9f8e7d6c' }])
    expect(fragment.inner).toBe('Старе резюме.')
  })

  test('replaceFragment оновлює маркер і вміст, не чіпаючи решту', () => {
    const updated = replaceFragment(TEXT, {
      layer: 'L0',
      sources: [{ file: 'overview/index.md', essenceCrc: 'aaaaaaaa', fileCrc: 'bbbbbbbb' }],
      content: 'Нове резюме.'
    })
    expect(updated).toContain('overview/index.md aaaaaaaa bbbbbbbb')
    expect(updated).toContain('Нове резюме.')
    expect(updated).not.toContain('Старе резюме.')
    expect(updated).toContain('# Індекс')
    expect(updated).toContain('## Решта')
    expect(extractFragment(updated).inner).toBe('Нове резюме.')
  })

  test('нема маркерів → extractFragment null, replaceFragment кидає', () => {
    expect(extractFragment('# Без маркерів\n')).toBeNull()
    expect(() => replaceFragment('# Без маркерів\n', { layer: 'L0', sources: [], content: 'x' })).toThrow(
      MARKER_NOT_FOUND_RE
    )
  })
})
