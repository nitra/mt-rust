import { describe, expect, test } from 'vitest'

import { createLlm, LlmError } from '../llm.mjs'

/**
 * Стаб runOneShot зі сценарієм відповідей і журналом викликів.
 * @param {Array<{content?: string, model?: string, error?: string}>} responses черга відповідей
 * @returns {{calls: object[], impl: {runOneShot: (request: object) => Promise<object>}}} журнал і injected-транспорт
 */
function stubTransport(responses) {
  const calls = []
  return {
    calls,
    impl: {
      /**
       * @param {object} request запит one-shot
       * @returns {Promise<object>} відповідь за сценарієм
       */
      runOneShot(request) {
        calls.push(request)
        return Promise.resolve(responses[Math.min(calls.length - 1, responses.length - 1)])
      }
    }
  }
}

describe('createLlm.generate', () => {
  test('успіх із першої спроби: повертає content і model', async () => {
    const { calls, impl } = stubTransport([{ content: 'готово', model: 'stub/m1' }])
    const llm = await createLlm({ tier: 'avg', caller: 'test', impl })
    const result = await llm.generate({ system: 's', user: 'u' })
    expect(result).toEqual({ content: 'готово', model: 'stub/m1' })
    expect(calls).toHaveLength(1)
    expect(calls[0].modelTier).toBe('avg')
    expect(calls[0].caller).toBe('test')
  })

  test('невалідний вихід: ретрай тим самим tier, потім ескалація avg→max', async () => {
    const { calls, impl } = stubTransport([
      { content: 'погано', model: 'stub/m1' },
      { content: 'знову погано', model: 'stub/m1' },
      { content: 'ВАЛІДНО', model: 'stub/m2' }
    ])
    const llm = await createLlm({ tier: 'avg', caller: 'test', impl })
    const result = await llm.generate({ system: 's', user: 'u', validate: c => c === 'ВАЛІДНО' })
    expect(result.model).toBe('stub/m2')
    expect(calls.map(call => call.modelTier)).toEqual(['avg', 'avg', 'max'])
  })

  test('усі спроби невалідні → LlmError code=output', async () => {
    const { impl } = stubTransport([{ content: 'погано', model: 'stub/m1' }])
    const llm = await createLlm({ tier: 'max', caller: 'test', impl })
    await expect(llm.generate({ system: 's', user: 'u', validate: () => false })).rejects.toMatchObject({
      code: 'output'
    })
  })

  test('транспортна помилка → LlmError code=unavailable без ретраїв', async () => {
    const { calls, impl } = stubTransport([{ error: 'ECONNREFUSED 127.0.0.1:8000' }])
    const llm = await createLlm({ tier: 'avg', caller: 'test', impl })
    await expect(llm.generate({ system: 's', user: 'u' })).rejects.toMatchObject({ code: 'unavailable' })
    expect(calls).toHaveLength(1)
  })

  test('chain: кожен виклик несе chain, end закриває з фактичним outcome', async () => {
    const ended = []
    const { calls, impl } = stubTransport([{ content: 'ок', model: 'stub/m1' }])
    impl.startChain = () => ({
      /** @param {object} result результат ланцюжка */
      end(result) {
        ended.push(result)
      }
    })
    const llm = await createLlm({ tier: 'avg', caller: 'test', chainKind: 'layers-test', chainUnit: 'x', impl })
    await llm.generate({ system: 's', user: 'u' })
    llm.end()
    expect(calls[0].chain).toBeDefined()
    expect(ended).toEqual([{ outcome: 'success' }])
  })

  test('LlmError зберігає code і message', () => {
    const error = new LlmError('unavailable', 'нема транспорту')
    expect(error.code).toBe('unavailable')
    expect(error.message).toContain('нема транспорту')
  })
})
