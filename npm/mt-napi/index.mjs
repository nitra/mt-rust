/**
 * `@7n/mt-napi` — публічний API для Node/Bun-споживачів `mt` без spawn на
 * кожен виклик (спека `docs/specs/2026-07-27-mt-napi-binding.md`).
 *
 * Кожна функція спершу пробує native napi-виклик (`crates/mt-napi`); якщо
 * аддон не резолвнувся для платформи — fallback на `spawnSync('mt', [...,
 * '--json'])` і `JSON.parse(stdout)` (рішення Е — той самий контракт, що й
 * `mt-js/bin/mt.js`, лише з парсингом JSON замість `stdio: 'inherit'`).
 */
import { spawnSync } from 'node:child_process'
import { env as procEnv } from 'node:process'

import { loadNative } from './native.mjs'

/** @returns {string} шлях або ім'я бінарника `mt` для запуску через `spawnSync` */
function mtBin() {
  return procEnv.MT_BIN || 'mt'
}

/**
 * @param {string[]} args аргументи `mt` без `--json`/`--root` (додаються тут)
 * @param {string | undefined} root значення `--root`, якщо задано
 * @returns {unknown} `JSON.parse`-ований stdout
 */
function runCliJson(args, root) {
  const fullArgs = [...args, '--json']
  if (root) fullArgs.push('--root', root)
  const result = spawnSync(mtBin(), fullArgs, { encoding: 'utf8' })
  if (result.error) {
    throw new Error(`mt CLI fallback: ${result.error.message}`)
  }
  if (result.status !== 0) {
    throw new Error(`mt CLI fallback (${fullArgs.join(' ')}): ${result.stderr.trim()}`)
  }
  return JSON.parse(result.stdout)
}

/** @returns {Record<string, unknown> | null} exports napi-аддона, або `null` якщо недоступний */
function native() {
  return loadNative()
}

/**
 * @param {string} name ім'я worktree (гілка `mt/<name>`)
 * @param {{ base?: string, root?: string }} [opts] `base` — вихідна гілка (типово `main`), `root` — корінь проєкту
 * @returns {{ path: string }} шлях до створеного worktree
 */
export function worktreeCreate(name, opts = {}) {
  const addon = native()
  if (addon) return addon.worktreeCreate(name, opts.base ?? null, opts.root ?? null)
  const args = ['worktree', 'create', name]
  if (opts.base) args.push('--base', opts.base)
  return runCliJson(args, opts.root)
}

/**
 * @param {string} name ім'я worktree для видалення
 * @param {{ force?: boolean, root?: string }} [opts] `force` — прибрати навіть брудне дерево, `root` — корінь проєкту
 * @returns {{ removed: string }} шлях видаленого worktree
 */
export function worktreeRemove(name, opts = {}) {
  const addon = native()
  if (addon) return addon.worktreeRemove(name, opts.force ?? null, opts.root ?? null)
  const args = ['worktree', 'remove', name]
  if (opts.force) args.push('--force')
  return runCliJson(args, opts.root)
}

/**
 * @param {{ root?: string }} [opts] `root` — корінь проєкту
 * @returns {Array<Record<string, unknown>>} worktree репо: вік, stale-прапор, матч на задачу
 */
export function worktreeStatus(opts = {}) {
  const addon = native()
  if (addon) return addon.worktreeStatus(opts.root ?? null)
  return runCliJson(['worktree', 'inventory'], opts.root)
}

/**
 * @param {{ root?: string }} [opts] `root` — корінь проєкту
 * @returns {Array<Record<string, unknown>>} дерево вузлів задач
 */
export function scan(opts = {}) {
  const addon = native()
  if (addon) return addon.scan(opts.root ?? null)
  return runCliJson(['scan'], opts.root)
}

/**
 * @param {string | undefined} name задача (за замовчуванням — з поточної директорії)
 * @param {{ mode?: 'agent' | 'human', root?: string }} [opts] `mode` — override виконавця плану, `root` — корінь проєкту
 * @returns {{ plan_file: string }} ім'я записаного файлу плану
 */
export function plan(name, opts = {}) {
  const addon = native()
  if (addon) return addon.plan(name ?? null, opts.mode ?? null, opts.root ?? null)
  const args = ['plan']
  if (name) args.push(name)
  if (opts.mode) args.push('--mode', opts.mode)
  return runCliJson(args, opts.root)
}

/**
 * @param {string | undefined} name задача (за замовчуванням — з поточної директорії)
 * @param {{ root?: string }} [opts] `root` — корінь проєкту
 * @returns {{ plan_file: string, nnn: number, decision: string | null, decided: boolean, children: unknown[] }} read-only стан plan-review
 */
export function spawnReview(name, opts = {}) {
  const addon = native()
  if (addon) return addon.spawnReview(name ?? null, opts.root ?? null)
  const args = ['spawn']
  if (name) args.push(name)
  return runCliJson(args, opts.root)
}

/**
 * @param {string | undefined} name задача (за замовчуванням — з поточної директорії)
 * @param {{ root?: string }} [opts] `root` — корінь проєкту
 * @returns {{ approved_file: string, children: string[] }} схвалений план і матеріалізовані діти
 */
export function spawnApprove(name, opts = {}) {
  const addon = native()
  if (addon) return addon.spawnApprove(name ?? null, opts.root ?? null)
  const args = ['spawn']
  if (name) args.push(name)
  args.push('--approve')
  return runCliJson(args, opts.root)
}

/**
 * @param {string | undefined} name задача (за замовчуванням — з поточної директорії)
 * @param {string} reason причина відхилення
 * @param {{ root?: string }} [opts] `root` — корінь проєкту
 * @returns {{ rejected_file: string }} ім'я файлу відхилення
 */
export function spawnReject(name, reason, opts = {}) {
  const addon = native()
  if (addon) return addon.spawnReject(name ?? null, reason, opts.root ?? null)
  const args = ['spawn']
  if (name) args.push(name)
  args.push('--reject', reason)
  return runCliJson(args, opts.root)
}
