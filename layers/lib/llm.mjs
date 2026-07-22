/** @see ./docs/llm.md */

import process from 'node:process'

/**
 * Помилка LLM-шару. `code`:
 * - `unavailable` — транспорт недоступний (нема llm-lib/pi, жодної моделі, мережа);
 * - `output` — модель відповідає, але вихід не пройшов валідацію після всіх спроб.
 */
export class LlmError extends Error {
  /**
   * @param {'unavailable' | 'output'} code клас відмови
   * @param {string} message людиночитне пояснення
   */
  constructor(code, message) {
    super(message)
    this.code = code
  }
}

/** Порядок ескалації тирів model-tiers llm-lib (кожен тир сам каскадить local→cloud). */
const TIERS = ['min', 'avg', 'max']

/**
 * @param {string} tier поточний тир
 * @returns {string | null} наступний (дорожчий) тир або null
 */
function nextTier(tier) {
  const index = TIERS.indexOf(tier)
  return index !== -1 && index < TIERS.length - 1 ? TIERS[index + 1] : null
}

/**
 * Тонка обгортка над `@7n/llm-lib`: one-shot із ретраєм і tier-ескалацією,
 * один chain на прогін. llm-lib вантажиться lazy — status ніколи його не тягне.
 * @param {object} options параметри прогону
 * @param {string} [options.tier] базовий tier моделей (min/avg/max)
 * @param {number} [options.maxTokens] стеля токенів відповіді
 * @param {number} [options.timeoutMs] стеля часу одного виклику (переклад повного тіла на local-моделі декодує довше за коротку суть; llm-lib дефолт 120с замало)
 * @param {string} options.caller ідентифікатор джерела для trace/telemetry
 * @param {string} [options.chainKind] kind ланцюжка (нема — без chain)
 * @param {string} [options.chainUnit] unit ланцюжка
 * @param {{runOneShot: (request: object) => Promise<{content?: string, model?: string, error?: string}>, startChain?: (options: object) => {end: (result: object) => void}}} [options.impl] injected-транспорт для тестів
 * @returns {Promise<{generate: (request: object) => Promise<{content: string, model: string}>, end: (outcome?: string) => void}>} готовий клієнт
 */
export async function createLlm({
  tier = 'avg',
  maxTokens = 4096,
  timeoutMs = 600_000,
  caller,
  chainKind,
  chainUnit,
  impl
}) {
  let runOneShot = impl?.runOneShot
  let startChain = impl?.startChain
  if (!runOneShot) {
    try {
      ;({ runOneShot } = await import('@7n/llm-lib/one-shot'))
      ;({ startChain } = await import('@7n/llm-lib/chain'))
    } catch (error) {
      throw new LlmError(
        'unavailable',
        `LLM-транспорт недоступний: не вдалось завантажити @7n/llm-lib (${error.message}). ` +
          'Перевір встановлення залежностей (bun install) і peer @earendil-works/pi-ai.'
      )
    }
  }
  const chain = chainKind && startChain ? startChain({ kind: chainKind, unit: chainUnit, cwd: process.cwd() }) : null
  let outcome = 'fail'

  return {
    /**
     * Генерація з валідацією: 1 ретрай тим самим tier → 1 ескалація tier-ом вище.
     * @param {object} request запит
     * @param {string} request.system системний промпт
     * @param {string} request.user користувацький промпт
     * @param {(content: string) => boolean} [request.validate] структурна перевірка виходу
     * @param {string} [request.tier] override базового tier для цього виклику
     * @returns {Promise<{content: string, model: string}>} валідний вихід і фактична модель
     */
    async generate({ system, user, validate, tier: tierOverride }) {
      const baseTier = tierOverride ?? tier
      const attempts = [baseTier, baseTier, nextTier(baseTier)].filter(Boolean)
      let lastModel = ''
      for (const attemptTier of attempts) {
        const result = await runOneShot({
          messages: [
            { role: 'system', content: system },
            { role: 'user', content: user }
          ],
          modelTier: attemptTier,
          maxTokens,
          timeoutMs,
          caller,
          ...(chain && { chain })
        })
        if (result.error) throw new LlmError('unavailable', `LLM-виклик не вдався (${attemptTier}): ${result.error}`)
        const content = (result.content ?? '').trim()
        lastModel = result.model ?? attemptTier
        if (content && (!validate || validate(content))) {
          outcome = 'success'
          return { content, model: lastModel }
        }
      }
      const escalation = nextTier(baseTier)
      const tierPath = escalation ? `${baseTier}→${escalation}` : baseTier
      throw new LlmError(
        'output',
        `Вихід моделі ${lastModel} не пройшов валідацію після ${attempts.length} спроб (tier ${tierPath})`
      )
    },

    /**
     * Закриває chain з фактичним результатом прогону.
     * @param {string} [finalOutcome] override результату (default — success після першої вдалої генерації)
     * @returns {void}
     */
    end(finalOutcome = outcome) {
      chain?.end({ outcome: finalOutcome })
    }
  }
}
