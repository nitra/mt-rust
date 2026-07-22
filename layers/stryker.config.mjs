/** @type {import('@stryker-mutator/core').PartialStrykerOptions} */
export default {
  testRunner: 'vitest',
  vitest: { configFile: 'vitest.config.mjs' },
  // perTest: Stryker запускає лише тести, що покривають мутовану лінію — головний приріст
  // швидкості проти command runner (де треба було б ганяти ввесь test-suite на кожен мутант).
  coverageAnalysis: 'perTest',
  tempDirName: 'reports/stryker/.tmp',
  reporters: ['json', 'clear-text'],
  jsonReporter: { fileName: 'reports/stryker/mutation.json' },
  // incremental: зберігає результати між запусками, відновлює після краш/kill.
  incremental: true,
  incrementalFile: 'reports/stryker/incremental.json'
}
