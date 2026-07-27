/**
 * Loader napi-аддона `mt` (Rust-ядро `crates/mt-napi` → `mt-core`).
 *
 * Порядок пошуку (спека `docs/specs/2026-07-27-mt-napi-binding.md`, рішення Е):
 *   1. `MT_NATIVE_ADDON` — явний override шляху до `.node`-файлу (dev/CI/тести).
 *   2. Platform-підпакет `@7n/mt-napi-<platform>-<arch>` (optionalDependency).
 *   3. Dev-fallback: `crates/mt-napi/mt.<triple>.node` (локальна збірка `napi build`).
 *
 * Лише для Bun (napi-контракт цієї ітерації — `.d.ts`/API не гарантує
 * сумісності з чистим Node.js).
 */
import { existsSync } from 'node:fs'
import { createRequire } from 'node:module'
import { dirname, join } from 'node:path'
import { arch as osArch, env as procEnv, platform as osPlatform } from 'node:process'
import { fileURLToPath } from 'node:url'

const require = createRequire(import.meta.url)
const HERE = dirname(fileURLToPath(import.meta.url))
/** Корінь репо: npm/mt-napi → up 2. */
const REPO_ROOT = join(HERE, '..', '..')

/** Platform-arch → { platformPackage, napiSuffix }. */
const TARGETS = {
  'darwin-arm64': { pkg: '@7n/mt-darwin-arm64', suffix: 'darwin-arm64' },
  'linux-x64': { pkg: '@7n/mt-linux-x64', suffix: 'linux-x64-musl' }
}

/** @type {Record<string, unknown> | null} */
let cached = null

/**
 * Резолвить шлях до napi-аддона `mt`.
 * @param {{
 *   env?: Record<string, string | undefined>,
 *   platform?: string,
 *   arch?: string,
 *   existsSync?: (p: string) => boolean,
 *   requireResolve?: (id: string) => string,
 *   repoRoot?: string
 * }} [deps] ін'єкції для тестів
 * @returns {string | null} шлях до файлу аддона, або `null` якщо не знайдено
 */
export function resolveNativeAddon(deps = {}) {
  const env = deps.env ?? procEnv
  const platform = deps.platform ?? osPlatform
  const arch = deps.arch ?? osArch
  const exists = deps.existsSync ?? existsSync
  const requireResolve = deps.requireResolve ?? (id => require.resolve(id))
  const repoRoot = deps.repoRoot ?? REPO_ROOT

  const override = env.MT_NATIVE_ADDON
  if (override) return override

  const target = TARGETS[`${platform}-${arch}`]
  if (target) {
    try {
      return requireResolve(`${target.pkg}/mt.${target.suffix}.node`)
    } catch {
      // платформний підпакет не встановлено — пробуємо dev-fallback
    }
    const devPath = join(repoRoot, 'crates', 'mt-napi', `mt.${target.suffix}.node`)
    if (exists(devPath)) return devPath
  }
  return null
}

/**
 * Кешований доступ до аддона (одне завантаження на процес). `null`, якщо
 * аддон не резолвнувся або `require()` кинув — виклик має впасти на CLI-fallback.
 * @param {{ resolve?: () => string | null, requireFn?: (p: string) => Record<string, unknown> }} [deps] ін'єкції для тестів
 * @returns {Record<string, unknown> | null} exports аддона, або `null` якщо не резолвнувся/не завантажився
 */
export function loadNative(deps = {}) {
  if (cached !== null) return cached
  const path = (deps.resolve ?? resolveNativeAddon)()
  if (!path) return null
  try {
    cached = (deps.requireFn ?? require)(path)
    return cached
  } catch {
    return null
  }
}
