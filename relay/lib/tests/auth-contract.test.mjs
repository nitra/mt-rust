/**
 * Контракт auth-провайдера relay: `verifySession(token) → {account_id}`
 * (stack.md, «Relay-інфраструктура»).
 *
 * Той самий підхід, що й у store-контракті: взаємозамінність реалізацій
 * доводиться однаковою ПОВЕДІНКОЮ, а не однаковим переліком методів, тому
 * набір параметризований фабрикою. Kratos-реалізація працює через
 * інʼєктований `fetch` — це перевіряє обробку документованої форми
 * відповіді `whoami`, а не сумісність із живим інстансом Kratos.
 */
import { describe, expect, test } from 'vitest'

import { DevMagicAuth, KratosAuth } from '../auth.mjs'
import { InMemoryStore } from '../store.mjs'

/**
 * Stub Kratos: віддає `whoami` за раніше зареєстрованими токенами.
 * @returns {{ fetch: Function, register: (token: string, session: object) => void }} клієнт і реєстратор сесій
 */
function kratosStub() {
  const sessions = new Map()
  return {
    register: (token, session) => sessions.set(token, session),
    fetch: async (url, options) => {
      expect(url).toMatch(/\/sessions\/whoami$/)
      const session = sessions.get(options?.headers?.['X-Session-Token'])
      if (!session) return { ok: false, status: 401, json: async () => ({}) }
      return { ok: true, status: 200, json: async () => session }
    }
  }
}

/**
 * Набір перевірок контракту.
 * @param {string} label назва реалізації у виводі
 * @param {() => {auth: object, tokenFor: (email: string) => Promise<string>}} makeAuth фабрика провайдера і видачі токена
 * @returns {void}
 */
function authContract(label, makeAuth) {
  describe(`auth-контракт: ${label}`, () => {
    test('валідна сесія повертає account_id, стабільний між викликами', async () => {
      const { auth, tokenFor } = makeAuth()
      const token = await tokenFor('a@x')
      const first = await auth.verifySession(token)
      expect(first.account_id).toBeTruthy()
      expect(await auth.verifySession(token)).toEqual(first)
    })

    test('той самий email — той самий акаунт', async () => {
      // Точка стикування провайдера з membership: account_id не має
      // «роздвоюватись» на повторному логіні, інакше учасник задачі стає
      // іншою людиною після перелогіну.
      const { auth, tokenFor } = makeAuth()
      const one = await auth.verifySession(await tokenFor('same@x'))
      const two = await auth.verifySession(await tokenFor('same@x'))
      expect(two.account_id).toBe(one.account_id)
    })

    test('невідомий токен відхиляється', async () => {
      const { auth } = makeAuth()
      await expect(auth.verifySession('не-токен')).rejects.toThrow(/сесія відхилена/)
    })

    test('порожній токен відхиляється', async () => {
      const { auth } = makeAuth()
      await expect(auth.verifySession('')).rejects.toThrow(/сесія відхилена/)
      await expect(auth.verifySession(undefined)).rejects.toThrow(/сесія відхилена/)
    })
  })
}

authContract('dev-magic', () => {
  const auth = new DevMagicAuth({ store: new InMemoryStore() })
  return { auth, tokenFor: async email => (await auth.issueSession(email)).token }
})

authContract('kratos', () => {
  const stub = kratosStub()
  const auth = new KratosAuth({ store: new InMemoryStore(), baseUrl: 'https://kratos.test/', fetch: stub.fetch })
  let seq = 0
  return {
    auth,
    tokenFor: async email => {
      seq += 1
      const token = `kratos-${seq}`
      stub.register(token, { active: true, identity: { traits: { email } } })
      return token
    }
  }
})

describe('dev-magic: час життя сесії', () => {
  test('прострочена сесія відхиляється так само, як невідома', async () => {
    // «Прострочено» і «невідомо» навмисно нерозрізнювані для викликача:
    // різні повідомлення дали б оракул на існування токена.
    let clock = 1000
    const auth = new DevMagicAuth({ store: new InMemoryStore(), ttlMs: 100, now: () => clock })
    const { token } = await auth.issueSession('ttl@x')
    expect(await auth.verifySession(token)).toBeTruthy()

    clock += 100
    await expect(auth.verifySession(token)).rejects.toThrow(/невідомий токен/)
    // Прострочений запис не лишається в памʼяті.
    expect(auth.sessions.size).toBe(0)
  })

  test('revokeSession завершує сесію', async () => {
    const auth = new DevMagicAuth({ store: new InMemoryStore() })
    const { token } = await auth.issueSession('bye@x')
    await auth.revokeSession(token)
    await expect(auth.verifySession(token)).rejects.toThrow(/сесія відхилена/)
  })

  test('логін без email відхиляється', async () => {
    const auth = new DevMagicAuth({ store: new InMemoryStore() })
    await expect(auth.issueSession('')).rejects.toThrow(/потрібен email/)
  })
})

describe('kratos: форма відповіді whoami', () => {
  /**
   * Створює провайдер із підготованою відповіддю whoami.
   * @param {object} session тіло відповіді
   * @returns {KratosAuth} провайдер
   */
  function withSession(session) {
    const stub = kratosStub()
    stub.register('t', session)
    return new KratosAuth({ store: new InMemoryStore(), baseUrl: 'https://kratos.test', fetch: stub.fetch })
  }

  test('неактивна сесія відхиляється', async () => {
    const auth = withSession({ active: false, identity: { traits: { email: 'x@x' } } })
    await expect(auth.verifySession('t')).rejects.toThrow(/неактивна/)
  })

  test('identity без email відхиляється', async () => {
    // Без email нема чим стикувати identity з membership — це відмова,
    // а не акаунт без адреси.
    const auth = withSession({ active: true, identity: { traits: {} } })
    await expect(auth.verifySession('t')).rejects.toThrow(/без email/)
  })

  test('traits.name як обʼєкт не потрапляє в display_name', async () => {
    // У Kratos `name` буває структурою {first, last}; наївне присвоєння
    // поклало б у профіль "[object Object]".
    const store = new InMemoryStore()
    const stub = kratosStub()
    stub.register('t', { active: true, identity: { traits: { email: 'n@x', name: { first: 'Ann' } } } })
    const auth = new KratosAuth({ store, baseUrl: 'https://kratos.test', fetch: stub.fetch })
    await auth.verifySession('t')
    expect((await store.accountByEmail('n@x')).display_name).toBe('')
  })

  test('слеш у кінці baseUrl не дублюється в шляху', async () => {
    // Перевіряє сам stub: він падає, якщо URL не закінчується whoami.
    const stub = kratosStub()
    stub.register('t', { active: true, identity: { traits: { email: 'slash@x' } } })
    const auth = new KratosAuth({ store: new InMemoryStore(), baseUrl: 'https://kratos.test///', fetch: stub.fetch })
    expect(await auth.verifySession('t')).toMatchObject({ account_id: expect.any(String) })
  })
})
