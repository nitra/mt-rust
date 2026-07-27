import { defineConfig } from 'vitest/config'

// Кореневий конфіг для `bunx --bun vitest run` (root workspace: relay, layers, npm/mt-napi).
// npm/mt-napi/contract.test.mjs резолвить нативний napi-аддон (crates/mt-napi) і CLI-бінарник
// (crates/mt) напряму з target/debug/ — обидва мають бути зібрані заздалегідь
// (`cargo build -p mt -p mt-napi`); тест сам пропускається (`describe.skipIf`), якщо їх немає.
export default defineConfig({
  test: {}
})
