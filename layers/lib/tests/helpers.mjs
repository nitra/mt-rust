import { mkdtempSync, rmSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'

import { currentCrcs } from '../build.mjs'
import { DRAFT_MARKER, formatSourceLine, serializeDoc } from '../md.mjs'

/**
 * Канон tmp-фікстур (test.mdc): ізольована тека, гарантоване прибирання,
 * без process.chdir.
 * @param {(dir: string) => Promise<void> | void} fn тіло тесту, отримує шлях ізольованої теки
 * @returns {Promise<void>} завершується після прибирання теки
 */
export async function withTmpDir(fn) {
  const dir = mkdtempSync(join(tmpdir(), 'layers-test-'))
  try {
    await fn(dir)
  } finally {
    rmSync(dir, { recursive: true, force: true })
  }
}

/**
 * Leaf-дока полігона.
 * @param {{essence?: string | null, details?: string, draft?: boolean}} [options] суть (null — без секції), деталі, прапор чернетки
 * @returns {string} вміст leaf-файлу
 */
export function leafDoc({ essence = 'Суть без змін.', details = 'Багато деталей.', draft = false } = {}) {
  const draftLine = draft ? `${DRAFT_MARKER}\n` : ''
  const essenceSection = essence === null ? '' : `## Суть\n\n${draftLine}${essence}\n\n`
  return `---\ntype: architecture\ndescription: тест\ntimestamp: 2026-07-12\n---\n\n# Глава\n\n${essenceSection}## Деталі\n\n${details}\n`
}

/**
 * Згенерована дока верхнього шару з коректними (актуальними) CRC джерел.
 * @param {{layer: string, title: string, sources: Record<string, string>, essence?: string}} options шар, назва, вміст джерел і суть доки
 * @returns {string} вміст згенерованої доки
 */
export function generatedDoc({ layer, title, sources, essence = 'Агрегована суть.' }) {
  const lines = Object.entries(sources).map(([file, text]) => {
    const { essenceCrc, fileCrc } = currentCrcs(text)
    return formatSourceLine({ file, essenceCrc, fileCrc })
  })
  const fm = {
    type: 'layered-doc',
    layer,
    title,
    timestamp: '2026-07-12',
    layers: { model: 'stub/model', sources: lines }
  }
  return serializeDoc(fm, `\n# ${title}\n\n## Суть\n\n${essence}\n\n## Розгортка\n\nТекст.\n`)
}

/**
 * In-memory readFile поверх Map.
 * @param {Record<string, string>} files мапа шлях → вміст
 * @returns {(rel: string) => string | null} readFile-сумісна функція
 */
export function memoryReader(files) {
  return rel => files[rel] ?? null
}
