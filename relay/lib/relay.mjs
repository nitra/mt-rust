/**
 * Ядро relay (M2, mission control): membership-гейти кімнат, ролі,
 * запрошення, transfer ownership, роздача pubkey-ів (access.md).
 *
 * Межі (access.md): relay координує і пересилає — НЕ зберігає журнали
 * сесій, НЕ проксіює git, НЕ видає lease (істина — git claim), НЕ виконує
 * агентів. Транспортний шар (WS) — server.mjs; тут — чиста логіка.
 */
import { Presence } from './presence.mjs'
import { Rooms } from './rooms.mjs'
import { transferMessage, verifySignature } from './signing.mjs'
import { roleAtLeast } from './store.mjs'

/** Ядро relay поверх store + rooms (+ опційний push-маршрутизатор). */
export class RelayCore {
  /**
   * @param {{ store: import('./store.mjs').InMemoryStore, rooms?: Rooms, push?: import('./push.mjs').PushRouter, auth?: object, presence?: Presence }} deps залежності
   */
  constructor({ store, rooms = new Rooms(), push = null, auth = null, presence = new Presence() }) {
    this.store = store
    this.rooms = rooms
    this.push = push
    this.auth = auth
    this.presence = presence
  }

  /**
   * Логін у dev-режимі: видає сесію на email без доставки листа.
   *
   * Доступний, лише якщо провайдер auth **уміє** видавати сесії — у
   * продакшні (Kratos) методу немає, тож шлях закритий за побудовою, а не
   * прапорцем конфігурації (auth.mjs).
   * @param {string} email email користувача
   * @returns {Promise<{token: string, account_id: string, expires_at: string}>} сесія
   * @throws {Error} провайдер не видає сесій
   */
  async devLogin(email) {
    if (typeof this.auth?.issueSession !== 'function') {
      throw new Error('login відхилено: провайдер auth не видає сесій')
    }
    return await this.auth.issueSession(email)
  }

  /**
   * Реєстрація пристрою за сесією користувача — ланка, якої бракувало між
   * «людина залогінилась» і «пристрій у кімнаті»: сесія доводить, чий це
   * акаунт, пристрій приносить свій Ed25519-pubkey і отримує `device_token`
   * (access.md, «Акаунти, пристрої, ключі»).
   * @param {string} sessionToken токен сесії
   * @param {{ name: string, role: string, pubkey: string }} params дані пристрою
   * @returns {Promise<{device_id: string, device_token: string}>} ідентифікатор і токен
   * @throws {Error} auth не налаштований, сесія невалідна або pubkey не hex-32
   */
  async registerDevice(sessionToken, { name, role, pubkey }) {
    if (!this.auth) throw new Error('реєстрація відхилена: auth-провайдер не налаштований')
    const { account_id } = await this.auth.verifySession(sessionToken)
    return await this.store.registerDevice(account_id, { name, role, pubkey })
  }

  /**
   * Записує registration token транспорту push для підключеного пристрою.
   *
   * Окремий крок після `hello`, а не поле реєстрації: FCM-токен видає сам
   * транспорт і змінює його самостійно (перевстановлення застосунку,
   * ротація), тож пристрій оновлює його стільки разів, скільки треба.
   * @param {object} device запис підключеного пристрою
   * @param {string} pushToken токен транспорту (порожній — зняти)
   * @returns {Promise<void>} завершення запису
   */
  async setPushToken(device, pushToken) {
    await this.store.setPushToken(device.device_id, pushToken)
  }

  /**
   * Авторизує WS-підключення за device_token.
   * @param {string} deviceToken токен пристрою
   * @returns {object} запис пристрою
   * @throws {Error} невідомий токен
   */
  async connectDevice(deviceToken) {
    const device = await this.store.deviceByToken(deviceToken)
    if (!device) throw new Error('invalid device token')
    // Саме через store, а не присвоєнням у повернутий обʼєкт: у SQLite це
    // відірваний від бази рядок, тож присвоєння губилось мовчки — запис
    // виживав лише в in-memory реалізації.
    const at = new Date().toISOString()
    await this.store.touchDevice(device.device_id, at)
    device.last_seen = at
    return device
  }

  /**
   * Підписка на кімнату задачі: дозволена лише пристроям акаунтів-учасників
   * кореня (access.md, «Membership прив'язане до кореневого вузла»).
   * @param {object} device запис пристрою
   * @param {string} root кореневий вузол задачі
   * @param {(frame: object) => void} send доставка кадрів пристрою
   * @returns {() => void} відписка
   * @throws {Error} не учасник
   */
  async subscribe(device, root, send) {
    const role = await this.store.memberRole(root, device.account_id)
    if (!role) throw new Error(`subscribe відхилено: акаунт не учасник задачі ${root}`)
    // accountId у підписці — не декор: push типу 1 будить лише офлайнових,
    // а «офлайн» визначається саме наявністю живої підписки акаунта.
    return this.rooms.subscribe(root, { deviceId: device.device_id, accountId: device.account_id, send })
  }

  /**
   * Оголошує присутність пристрою в кімнаті (access.md: «presence (хости:
   * hostname, проєкти, активні вузли)») і транслює зміну учасникам.
   *
   * Presence не є правом на запис: «хто пише» лишається за git claim
   * (overview.md, принцип 1). Тому гейт тут той самий, що й на підписку —
   * членство, — і нічого більше; viewer теж присутній, бо «хто дивиться»
   * так само корисно бачити.
   * @param {object} device запис пристрою
   * @param {string} root кореневий вузол задачі
   * @param {{ hostname?: string, projects?: string[], nodes?: string[] }} info що оголошує пристрій
   * @returns {Promise<object>} запис присутності
   * @throws {Error} не учасник
   */
  async announcePresence(device, root, info = {}) {
    const role = await this.store.memberRole(root, device.account_id)
    if (!role) throw new Error(`presence відхилено: акаунт не учасник задачі ${root}`)
    const record = this.presence.announce(root, {
      deviceId: device.device_id,
      accountId: device.account_id,
      role: device.role,
      hostname: info.hostname,
      projects: info.projects,
      nodes: info.nodes
    })
    this.rooms.publish(root, {
      kind: 'event',
      event: { type: 'PresenceChanged', ...record }
    })
    return record
  }

  /**
   * Знімає присутність пристрою (закриття сокета/відписка) і транслює це.
   * Тихо нічого не робить, якщо пристрій не був присутній — закриття
   * сокета приходить і для тих, хто нічого не оголошував.
   * @param {object} device запис пристрою
   * @param {string} root кореневий вузол задачі
   * @returns {void}
   */
  dropPresence(device, root) {
    if (!this.presence.forget(root, device.device_id)) return
    this.rooms.publish(root, {
      kind: 'event',
      event: { type: 'PresenceChanged', device_id: device.device_id, account_id: device.account_id, gone: true }
    })
  }

  /**
   * Присутні в кімнаті — лише учасникам (той самий гейт, що й на підписку).
   * @param {object} device запис пристрою-запитувача
   * @param {string} root кореневий вузол задачі
   * @returns {Promise<object[]>} записи присутності
   * @throws {Error} не учасник
   */
  async presenceOf(device, root) {
    if (!(await this.store.memberRole(root, device.account_id))) {
      throw new Error('presence відхилено: акаунт не учасник задачі')
    }
    return this.presence.of(root)
  }

  /**
   * Клієнтський Envelope у кімнату. Viewer НЕ шле клієнтські події
   * (access.md: «relay відхиляє клієнтські події viewer-а, включно з
   * CancelTurn»); host+ і approver шлють (approver — ApprovalResponse).
   * Кадр отримує `from_host` за роллю ПРИСТРОЮ (не з кадру клієнта —
   * спуфінг виключено): host-ехо несе seq, який призначає хост; тонкі
   * клієнти рендерять лише host-кадри, а міст хоста ігнорує їх (анти-цикл).
   * @param {object} device запис пристрою
   * @param {string} root кореневий вузол задачі
   * @param {object} envelope конверт (opaque — далі роутінгових полів не парситься)
   * @returns {void}
   * @throws {Error} viewer або не учасник
   */
  async clientEnvelope(device, root, envelope) {
    const role = await this.store.memberRole(root, device.account_id)
    if (!role) throw new Error(`envelope відхилено: акаунт не учасник задачі ${root}`)
    if (!roleAtLeast(role, 'approver')) {
      throw new Error('envelope відхилено: роль viewer не шле клієнтські події')
    }
    this.rooms.publish(root, { kind: 'envelope', envelope, from_host: device.role === 'host' })
    // Тип 3 push («потребує уваги») — з роутінгових полів події (push.mjs).
    await this.push?.onEnvelope(root, envelope, device.account_id)
  }

  /**
   * Запрошення учасника (лише owner). Push отримувачу — тип 2 (push.mjs).
   * @param {string} ownerAccount акаунт-запрошувач
   * @param {string} root кореневий вузол задачі
   * @param {{ email: string, role: string }} params кого і з якою роллю
   * @returns {object} запис запрошення (status: pending)
   * @throws {Error} не owner
   */
  async invite(ownerAccount, root, { email, role }) {
    if ((await this.store.memberRole(root, ownerAccount)) !== 'owner') {
      throw new Error('invite відхилено: запрошує лише owner')
    }
    const invitation = await this.store.createInvitation(root, ownerAccount, email, role)
    // Тип 2 push «вас запрошено»; незареєстрований email — pending мовчки.
    await this.push?.invited(email, root)
    return invitation
  }

  /**
   * Прийняття запрошення: запис у task_members + broadcast MemberChanged
   * у кімнату (access.md, «Membership API relay»).
   * @param {string} invitationId id запрошення
   * @param {string} accountId акаунт, що приймає (email мусить збігатись)
   * @returns {{root_node_hash: string, role: string}} членство
   * @throws {Error} невідоме/не pending/чужий email
   */
  async accept(invitationId, accountId) {
    const invitation = await this.store.invitationById(invitationId)
    if (!invitation || invitation.status !== 'pending') {
      throw new Error('accept відхилено: запрошення не існує або вже оброблене')
    }
    const account = await this.store.accountById(accountId)
    if (!account || account.email !== invitation.to_email) {
      throw new Error('accept відхилено: запрошення адресоване іншому акаунту')
    }
    await this.store.setInvitationStatus(invitationId, 'accepted')
    await this.store.setMemberRole(invitation.root_node_hash, accountId, invitation.role)
    this.rooms.publish(invitation.root_node_hash, {
      kind: 'event',
      event: { type: 'MemberChanged', account_id: accountId, role: invitation.role }
    })
    return { root_node_hash: invitation.root_node_hash, role: invitation.role }
  }

  /**
   * Відхилення запрошення отримувачем.
   * @param {string} invitationId id запрошення
   * @param {string} accountId акаунт, що відхиляє
   * @returns {void}
   * @throws {Error} невідоме/чужий email
   */
  async decline(invitationId, accountId) {
    const invitation = await this.store.invitationById(invitationId)
    const account = await this.store.accountById(accountId)
    if (!invitation || !account || account.email !== invitation.to_email) {
      throw new Error('decline відхилено: запрошення не існує або адресоване іншому')
    }
    await this.store.setInvitationStatus(invitationId, 'declined')
  }

  /**
   * Transfer ownership: поточний owner передає роль; сам стає host
   * (штатний шлях succession — access.md). Мережевий шлях (WS) додатково
   * вимагає Ed25519-підпис canonical-акта пристроєм-ініціатором — передача
   * власності стає криптографічним фактом, а не лише правом токена;
   * прямий виклик без signed — локальний/адміністративний шлях.
   * @param {string} root кореневий вузол задачі
   * @param {string} fromAccount поточний owner
   * @param {string} toAccount новий owner (мусить бути учасником)
   * @param {{ device: object, signature: string }} [signed] підписаний акт (WS-шлях)
   * @returns {void}
   * @throws {Error} не owner / отримувач не учасник / невалідний підпис
   */
  async transferOwnership(root, fromAccount, toAccount, signed) {
    if ((await this.store.memberRole(root, fromAccount)) !== 'owner') {
      throw new Error('transfer відхилено: передає лише owner')
    }
    if (!(await this.store.memberRole(root, toAccount))) {
      throw new Error('transfer відхилено: отримувач не учасник задачі')
    }
    if (signed) {
      const message = transferMessage({ root, fromAccount, toAccount })
      if (!verifySignature(signed.device.pubkey, message, signed.signature)) {
        throw new Error('transfer відхилено: підпис акта не пройшов перевірку')
      }
    }
    await this.store.setMemberRole(root, toAccount, 'owner')
    await this.store.setMemberRole(root, fromAccount, 'host')
    this.rooms.publish(root, {
      kind: 'event',
      event: { type: 'MemberChanged', account_id: toAccount, role: 'owner' }
    })
  }

  /**
   * Bootstrap членства з `owner:`-розмітки лісу (спека owner-app 260714):
   * власник кореня подає перелік {email, role} — зареєстровані акаунти
   * стають учасниками одразу, незареєстровані отримують pending-запрошення.
   * Ідемпотентно: наявні ролі не змінюються (не понижуємо і не дублюємо),
   * повторний прогін не плодить запрошення.
   * @param {string} ownerAccount акаунт-ініціатор (owner кореня)
   * @param {string} root кореневий вузол задачі
   * @param {{ email: string, role?: string }[]} entries учасники з розмітки
   * @returns {{ added: string[], invited: string[], kept: string[] }} підсумок за email
   * @throws {Error} не owner
   */
  async bootstrapMembers(ownerAccount, root, entries) {
    if ((await this.store.memberRole(root, ownerAccount)) !== 'owner') {
      throw new Error('bootstrap відхилено: сідить membership лише owner')
    }
    const result = { added: [], invited: [], kept: [] }
    for (const { email, role = 'owner' } of entries ?? []) {
      const account = await this.store.accountByEmail(email)
      if (account) {
        if (await this.store.memberRole(root, account.account_id)) {
          result.kept.push(email)
          continue
        }
        await this.store.setMemberRole(root, account.account_id, role)
        this.rooms.publish(root, {
          kind: 'event',
          event: { type: 'MemberChanged', account_id: account.account_id, role }
        })
        result.added.push(email)
        continue
      }
      if (!(await this.store.pendingInvitationFor(root, email))) {
        await this.store.createInvitation(root, ownerAccount, email, role)
        await this.push?.invited(email, root)
      }
      result.invited.push(email)
    }
    return result
  }

  /**
   * Pubkey-и пристроїв учасників approver+ — для перевірки підписів
   * approvals хостом. Доступ лише пристроям учасників (access.md).
   * @param {object} device запис пристрою-запитувача
   * @param {string} root кореневий вузол задачі
   * @returns {{device_id: string, account_id: string, pubkey: string}[]} pubkey-и
   * @throws {Error} не учасник
   */
  async pubkeys(device, root) {
    if (!(await this.store.memberRole(root, device.account_id))) {
      throw new Error('pubkeys відхилено: акаунт не учасник задачі')
    }
    return await this.store.pubkeysFor(root)
  }
}
