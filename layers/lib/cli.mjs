/** @see ./docs/cli.md */

import { mkdirSync, readFileSync, writeFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import process from 'node:process'

import { bootstrapEssences, refreshFileCrcs, runBuild } from './build.mjs'
import { loadConfig } from './layers.mjs'
import { createLlm } from './llm.mjs'
import { computeStatus, renderStatus } from './status.mjs'
import { runTranslate } from './translate.mjs'

const USAGE = `Використання: layers <команда> <docsDir> [опції]

Команди:
  status      стан шарів і перекладів (детерміновано, без мережі)
  refresh     підтвердити details-only: переписати fileCrc джерел без перебудови
  bootstrap   згенерувати чернетки «## Суть» для leaf-док без секції (LLM)
  build       перебудувати застарілі доки верхніх шарів із сутей джерел (LLM)
  translate   згенерувати/оновити derived-переклади док scope (LLM)

Опції:
  --json                машиночитний вивід (status)
  --strict              details-only теж вважати за розсинхрон (status)
  --dry-run             показати кандидатів, нічого не писати (build, translate)
  --force               перебудувати всі доки незалежно від стану (build)
  --only <файл>         обмежитись однією докою (build, translate)
  --lang <код>          обмежитись однією мовою (translate)
  --with-translations   після build одразу прогнати translate

Exit codes: 0 — свіжо/успіх · 1 — є розсинхрон або частина не збудувалась ·
2 — структурна проблема (конфіг, no-essence) · 3 — LLM недоступна
`

/** Опції, що очікують значення наступним аргументом. */
const VALUE_FLAGS = new Set(['--only', '--lang'])

/**
 * @param {string[]} argv аргументи після імені скрипта
 * @returns {{command?: string, docsDir?: string, flags: Record<string, string | boolean>}} розібрані аргументи
 */
export function parseArgv(argv) {
  /** @type {Record<string, string | boolean>} */
  const flags = {}
  const positional = []
  for (let index = 0; index < argv.length; index++) {
    const arg = argv[index]
    if (VALUE_FLAGS.has(arg)) {
      flags[arg.slice(2)] = argv[++index] ?? ''
    } else if (arg.startsWith('--')) {
      flags[arg.slice(2)] = true
    } else {
      positional.push(arg)
    }
  }
  return { command: positional[0], docsDir: positional[1], flags }
}

/**
 * Фабрика FS-доступу, замкнена на docsDir; читання відсутнього файлу → null.
 * @param {string} docsDir корінь полігона документації
 * @returns {{readFile: (rel: string) => string | null, writeFile: (rel: string, text: string) => void}} io-пара
 */
export function createIo(docsDir) {
  return {
    /**
     * @param {string} rel відносний шлях
     * @returns {string | null} вміст або null
     */
    readFile(rel) {
      try {
        return readFileSync(join(docsDir, rel), 'utf8')
      } catch {
        return null
      }
    },
    /**
     * @param {string} rel відносний шлях
     * @param {string} text новий вміст
     * @returns {void}
     */
    writeFile(rel, text) {
      const target = join(docsDir, rel)
      mkdirSync(dirname(target), { recursive: true })
      writeFileSync(target, text)
    }
  }
}

/**
 * LLM-команди створюють клієнт під прогін і закривають chain у finally.
 * @param {object} context контекст виконання
 * @param {ReturnType<typeof loadConfig>} context.config конфіг полігона
 * @param {ReturnType<typeof createIo>} context.io доступ до файлів
 * @param {string} context.command імʼя команди (для caller/chain)
 * @param {Record<string, string | boolean>} context.flags CLI-опції
 * @param {(text: string) => void} context.log вивід
 * @param {object | undefined} context.llmImpl injected-транспорт для тестів
 * @returns {Promise<number>} exit code
 */
async function runLlmCommand({ config, io, command, flags, log, llmImpl }) {
  const llm = await createLlm({
    tier: config.tier,
    maxTokens: config.maxTokens,
    caller: `layers:${command}`,
    chainKind: `layers-${command}`,
    chainUnit: config.docsDir,
    impl: llmImpl
  })
  const today = new Date().toISOString().slice(0, 10)
  const shared = { io, llm, log, today }
  try {
    if (command === 'bootstrap') {
      const { drafted, skipped, failed } = await bootstrapEssences(config, shared)
      log(`Чернеток: ${drafted.length} · пропущено (суть є): ${skipped.length} · помилок: ${failed.length}`)
      for (const failure of failed) log(`  ✗ ${failure.file}: ${failure.reason}`)
      return failed.length ? 1 : 0
    }
    const buildOptions = {
      ...shared,
      dryRun: flags['dry-run'] === true,
      force: flags.force === true,
      only: typeof flags.only === 'string' ? flags.only : undefined
    }
    if (command === 'build') {
      const { built, failed } = await runBuild(config, buildOptions)
      log(`Збудовано: ${built.length} · помилок: ${failed.length}`)
      for (const failure of failed) log(`  ✗ ${failure.file}: ${failure.reason}`)
      if (!failed.length && flags['with-translations'] === true) {
        const translated = await runTranslate(config, buildOptions)
        log(`Перекладів: ${translated.written.length} · помилок: ${translated.failed.length}`)
        return translated.failed.length ? 1 : 0
      }
      return failed.length ? 1 : 0
    }
    const { written, skipped, failed } = await runTranslate(config, {
      ...buildOptions,
      lang: typeof flags.lang === 'string' ? flags.lang : undefined
    })
    log(`Перекладів: ${written.length} · свіжих пропущено: ${skipped.length} · помилок: ${failed.length}`)
    for (const failure of failed) log(`  ✗ ${failure.file}: ${failure.reason}`)
    return failed.length ? 1 : 0
  } finally {
    llm.end()
  }
}

/**
 * @param {string[]} argv аргументи після імені скрипта
 * @param {{log?: (text: string) => void, llmImpl?: object}} [deps] залежності (тести)
 * @returns {Promise<number>} exit code
 */
export async function main(argv, { log = globalThis.console.log, llmImpl } = {}) {
  const { command, docsDir, flags } = parseArgv(argv)
  if (!command || !docsDir) {
    log(USAGE)
    return command ? 2 : 0
  }
  let config
  try {
    config = loadConfig(docsDir)
  } catch (error) {
    log(String(error.message ?? error))
    return 2
  }
  const io = createIo(docsDir)

  if (command === 'status') {
    const report = computeStatus(config, io.readFile)
    const { text, exitCode } = renderStatus(report, {
      json: flags.json === true,
      strict: flags.strict === true
    })
    log(text)
    return exitCode
  }

  if (command === 'refresh') {
    const updated = refreshFileCrcs(config, io)
    log(updated.length ? `Оновлено fileCrc у: ${updated.join(', ')}` : 'Нічого оновлювати — details-only відсутні')
    return 0
  }

  if (command === 'bootstrap' || command === 'build' || command === 'translate') {
    try {
      return await runLlmCommand({ config, io, command, flags, log, llmImpl })
    } catch (error) {
      log(String(error.message ?? error))
      return error.code === 'unavailable' ? 3 : 1
    }
  }

  log(`Невідома команда '${command}'\n\n${USAGE}`)
  return 2
}

if (process.argv[1] && import.meta.url.endsWith(process.argv[1].split('/').pop() ?? '')) {
  process.exitCode = await main(process.argv.slice(2))
}
