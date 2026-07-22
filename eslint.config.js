import { getConfig } from '@nitra/eslint-config'

export default [
  {
    ignores: [
      '**/auto-imports.d.ts',
      'docs/**',
      // Згенеровані артефакти (gitignored): coverage report і Stryker mutation sandbox/output.
      '**/coverage/**',
      '**/reports/stryker/**'
    ]
  },
  ...getConfig(),
  // Тест-хелпери не потребують JSDoc. `jsdoc/require-jsdoc` (warning) автофіксом вставляє
  // порожні `/** */` заглушки, які oxlint (`jsdoc/require-param`/`require-returns`, deny)
  // потім відхиляє → `bun run lint` неідемпотентний (oxlint --fix && eslint --fix). Вимикаємо
  // presence-вимогу для тестів; повноту наявних JSDoc усе одно стереже oxlint.
  {
    files: ['**/*.test.{js,mjs,cjs}', '**/tests/**/*.{js,mjs,cjs}'],
    rules: {
      'jsdoc/require-jsdoc': 'off'
    }
  }
]
