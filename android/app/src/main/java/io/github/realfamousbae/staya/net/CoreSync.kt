package io.github.realfamousbae.staya.net

import uniffi.staya_core.CoreEvent
import uniffi.staya_core.Location
import uniffi.staya_core.StayaCore

/**
 * Сеть вокруг ядра по правилам protocol §8.2: ядро готовит и разбирает JSON,
 * здесь — только запросы. Блокирующий, не на главном потоке.
 */
class CoreSync(private val core: StayaCore, private val http: StayaClient) {
    private fun now() = System.currentTimeMillis() / 1000

    /** Пополняет одноразовые ключи на сервере (§4.3). */
    fun publishKeys() {
        val count = COUNT.find(http.get("/v1/keys/count"))?.groupValues?.get(1)?.toInt()
            ?: throw java.io.IOException("bad key count")
        val json = core.keysToPublish(count.toUInt(), now()) ?: return
        http.put("/v1/keys", json)
        core.markKeysPublished()
    }

    fun accept(uri: String) {
        // Новый аккаунт берёт сервер из приглашения (protocol §5.3).
        core.setServerFromLink(uri)
        val info = core.parseInvite(uri)
        // ID — base64url без выравнивания: экранировать в JSON нечего.
        val claimed = http.post("/v1/keys/claim", "{\"account_id\":\"${info.accountId}\"}")
        core.acceptInvite(uri, claimed, now())
        flush()
    }

    /** Удаления слотов, затем один POST со всеми конвертами, затем `completeSend`. */
    fun flush() {
        val batch = core.pendingSends()
        for (friend in batch.deleteSlots) {
            http.delete("/v1/slots/$friend")
            core.markSlotDeleted(friend)
        }
        val json = batch.requestJson ?: return
        core.completeSend(batch.ids, http.post("/v1/envelopes", json))
    }

    /** Забирает ящик, обрабатывает, подтверждает и отправляет ответы. */
    fun sync(): List<CoreEvent> {
        val processed = core.processMailbox(http.get("/v1/mailbox"), now())
        processed.ackJson?.let { http.post("/v1/mailbox/ack", it) }
        flush()
        return processed.events
    }

    /** Одно событие WebSocket: обработать, подтвердить, отправить ответы. */
    fun handleWsEvent(json: String): List<CoreEvent> {
        val processed = core.processWsEvent(json, now())
        processed.ackJson?.let { http.post("/v1/mailbox/ack", it) }
        flush()
        return processed.events
    }

    fun share(location: Location) {
        core.prepareLocationUpdate(location, now())
        flush()
    }

    private companion object {
        val COUNT = Regex("\"one_time_keys\"\\s*:\\s*(\\d+)")
    }
}
