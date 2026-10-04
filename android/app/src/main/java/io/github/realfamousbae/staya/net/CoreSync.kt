package io.github.realfamousbae.staya.net

import org.json.JSONObject
import uniffi.staya_core.CoreEvent
import uniffi.staya_core.Location
import uniffi.staya_core.StayaCore

/**
 * Сеть вокруг ядра по правилам protocol §8.2: ядро готовит и разбирает JSON,
 * здесь — только запросы. Блокирующий, не на главном потоке.
 */
class CoreSync(private val core: StayaCore, private val http: StayaHttp) {
    private fun now() = System.currentTimeMillis() / 1000

    /** Пополняет одноразовые ключи на сервере (§4.3). */
    fun publishKeys() {
        val count = JSONObject(http.get("/v1/keys/count")).getInt("one_time_keys")
        val json = core.keysToPublish(count.toUInt(), now()) ?: return
        http.put("/v1/keys", json)
        core.markKeysPublished()
    }

    fun accept(uri: String) {
        val info = core.parseInvite(uri)
        val claimed = http.post("/v1/keys/claim", JSONObject().put("account_id", info.accountId).toString())
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

    fun share(location: Location) {
        core.prepareLocationUpdate(location, now())
        flush()
    }
}
