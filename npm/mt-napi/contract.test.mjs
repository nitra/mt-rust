/**
 * Contract-тест: napi-виклик `scan()` vs `mt --json scan` мають повертати ту
 * саму інформацію (спека `docs/specs/2026-07-27-mt-napi-binding.md`, §2.З
 * "regression-safety"). Поля порівнюються після нормалізації camelCase↔snake_case
 * і `undefined`→`null` (napi-rs мапить `Option::None` у `undefined`, CLI-JSON —
 * у `null`; це очікувана різниця конвенцій, не розбіжність даних).
 *
 * Потребує заздалегідь зібраних `target/debug/mt` і `target/debug/libmt_napi.*`
 * (`cargo build -p mt -p mt-napi`) — пропускається, якщо їх немає (локальний dev
 * без Rust-тулчейну).
 */
import { execFileSync } from 'node:child_process'
import { createRequire } from 'node:module'
import { copyFileSync, existsSync, mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir, platform } from 'node:os'
import { join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { describe, expect, it } from 'vitest'

const nodeRequire = createRequire(import.meta.url)
const REPO_ROOT = fileURLToPath(new URL('../..', import.meta.url))
const CLI_BIN = join(REPO_ROOT, 'target', 'debug', 'mt')
const CDYLIB = join(
  REPO_ROOT,
  'target',
  'debug',
  platform() === 'darwin' ? 'libmt_napi.dylib' : 'libmt_napi.so'
)

/**
 * `require()`/Bun-лоадер розпізнає нативний addon лише за розширенням `.node`
 * (той самий контракт, що й production `native.mjs`) — сирий `.dylib`/`.so`
 * Bun намагається розпарсити як JS-текст. Копіюємо у тимчасовий `.node`-файл.
 * @param {string} cdylibPath шлях до зібраного `libmt_napi.dylib`/`.so`
 * @returns {Record<string, unknown>} exports аддона
 */
function loadAddon(cdylibPath) {
  const dest = join(tmpdir(), `mt-napi-contract-${process.pid}.node`)
  copyFileSync(cdylibPath, dest)
  // Нативний addon: шлях обчислюється (tmpdir()+pid), не зовнішній ввід.
  return nodeRequire(dest)
}

const toCamel = s => s.replaceAll(/_([a-z])/g, (_, c) => c.toUpperCase())

/**
 * Нормалізує CLI-JSON (snake_case, explicit null) до napi-форми (camelCase, undefined-omit).
 * @param {unknown} value значення з `JSON.parse` CLI-виводу
 * @returns {unknown} те саме значення, нормалізоване під napi-конвенції
 */
function normalizeCli(value) {
  if (Array.isArray(value)) return value.map(v => normalizeCli(v))
  if (value === null) return
  if (value !== null && typeof value === 'object') {
    const out = {}
    for (const [k, v] of Object.entries(value)) {
      const normalized = normalizeCli(v)
      if (normalized !== undefined) out[toCamel(k)] = normalized
    }
    return out
  }
  return value
}

const haveBuild = existsSync(CLI_BIN) && existsSync(CDYLIB)

describe.skipIf(!haveBuild)('napi vs CLI contract: scan', () => {
  it('returns the same task tree', () => {
    const root = mkdtempSync(join(tmpdir(), 'mt-napi-contract-'))
    try {
      writeFileSync(join(root, '.mt.json'), '{"mt_dir": "./mt"}\n')
      const taskDir = join(root, 'mt', 'demo')
      mkdirSync(taskDir, { recursive: true })
      writeFileSync(join(taskDir, 'task.md'), '---\nschema_version: 1\n---\n\n## Task\n')
      writeFileSync(join(taskDir, 'h.md'), '')

      const cliRaw = execFileSync(CLI_BIN, ['--root', root, 'scan', '--json'], { encoding: 'utf8' })
      const cliResult = normalizeCli(JSON.parse(cliRaw))

      const addon = loadAddon(CDYLIB)
      const napiResult = addon.scan(root)

      expect(napiResult).toEqual(cliResult)
    } finally {
      rmSync(root, { recursive: true, force: true })
    }
  })
})
