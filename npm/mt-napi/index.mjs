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

function mtBin() {
  return procEnv.MT_BIN || 'mt'
}

/**
 * @param {string[]} args
 * @param {string | undefined} root
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

/** @returns {Record<string, unknown> | null} */
function native() {
  return loadNative()
}

/**
 * @param {string} name
 * @param {{ base?: string, root?: string }} [opts]
 * @returns {{ path: string }}
 */
export function worktreeCreate(name, opts = {}) {
  const addon = native()
  if (addon) return addon.worktreeCreate(name, opts.base ?? null, opts.root ?? null)
  const args = ['worktree', 'create', name]
  if (opts.base) args.push('--base', opts.base)
  return runCliJson(args, opts.root)
}

/**
 * @param {string} name
 * @param {{ force?: boolean, root?: string }} [opts]
 * @returns {{ removed: string }}
 */
export function worktreeRemove(name, opts = {}) {
  const addon = native()
  if (addon) return addon.worktreeRemove(name, opts.force ?? null, opts.root ?? null)
  const args = ['worktree', 'remove', name]
  if (opts.force) args.push('--force')
  return runCliJson(args, opts.root)
}

/**
 * @param {{ root?: string }} [opts]
 * @returns {Array<Record<string, unknown>>}
 */
export function worktreeStatus(opts = {}) {
  const addon = native()
  if (addon) return addon.worktreeStatus(opts.root ?? null)
  return runCliJson(['worktree', 'inventory'], opts.root)
}

/**
 * @param {{ root?: string }} [opts]
 * @returns {Array<Record<string, unknown>>}
 */
export function scan(opts = {}) {
  const addon = native()
  if (addon) return addon.scan(opts.root ?? null)
  return runCliJson(['scan'], opts.root)
}

/**
 * @param {string | undefined} name
 * @param {{ mode?: 'agent' | 'human', root?: string }} [opts]
 * @returns {{ plan_file: string }}
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
 * @param {string | undefined} name
 * @param {{ root?: string }} [opts]
 * @returns {{ plan_file: string, nnn: number, decision: string | null, decided: boolean, children: unknown[] }}
 */
export function spawnReview(name, opts = {}) {
  const addon = native()
  if (addon) return addon.spawnReview(name ?? null, opts.root ?? null)
  const args = ['spawn']
  if (name) args.push(name)
  return runCliJson(args, opts.root)
}

/**
 * @param {string | undefined} name
 * @param {{ root?: string }} [opts]
 * @returns {{ approved_file: string, children: string[] }}
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
 * @param {string | undefined} name
 * @param {string} reason
 * @param {{ root?: string }} [opts]
 * @returns {{ rejected_file: string }}
 */
export function spawnReject(name, reason, opts = {}) {
  const addon = native()
  if (addon) return addon.spawnReject(name ?? null, reason, opts.root ?? null)
  const args = ['spawn']
  if (name) args.push(name)
  args.push('--reject', reason)
  return runCliJson(args, opts.root)
}
