import { defineConfig } from 'vitest/config'

// Кореневий конфіг для `bunx --bun vitest run` (root workspace: relay, crates/mt-napi).
// У монорепо nitra/mt тут був globalSetup, що збирав mt-scanner (npm/ scanner-shim) +
// mt-napi addon — npm/ (тонкий JS-клієнт) переїхав у nitra/mt-js і не входить у mt-rust,
// тож npm-специфічний setup прибрано. Якщо тести crates/mt-napi почнуть резолвити
// нативний addon напряму, додай localный globalSetup, що будує його через
// `cargo build -p mt-napi` (dev-fallback шукає target/{release,debug}/libmt_napi.*).
export default defineConfig({
  test: {}
})
