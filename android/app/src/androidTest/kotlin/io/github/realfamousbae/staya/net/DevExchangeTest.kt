package io.github.realfamousbae.staya.net

import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import io.github.realfamousbae.staya.location.Fix
import io.github.realfamousbae.staya.location.LocationSender
import java.io.File
import org.junit.Assert.assertTrue
import org.junit.Assume.assumeTrue
import org.junit.Test
import org.junit.runner.RunWith
import uniffi.staya_core.CoreEvent
import uniffi.staya_core.StayaCore

/**
 * Обмен позициями с тестовым собеседником (`tools/dev-peer --login`) через
 * настоящий сервер Staya с PostgreSQL на хосте CI (задача 4.1b): регистрация,
 * вход, ключи, приглашение, позиции. Аргументы инструментации: `server` — адрес
 * сервера для эмулятора (`http://10.0.2.2:8080`), `invite` — приглашение
 * собеседника в base64url (в `am instrument` нельзя передать `&` и `=`). Без
 * них тест пропускается. База — временная, не база приложения.
 */
@RunWith(AndroidJUnit4::class)
class DevExchangeTest {
    @Test
    fun exchangesLocationsWithPeer() {
        val args = InstrumentationRegistry.getArguments()
        val server = args.getString("server")
        val inviteB64 = args.getString("invite")
        assumeTrue("no server/invite arguments", server != null && inviteB64 != null)
        val invite = String(android.util.Base64.decode(inviteB64, android.util.Base64.URL_SAFE), Charsets.UTF_8)
        val context = InstrumentationRegistry.getInstrumentation().targetContext
        val db = File(context.cacheDir, "dev-exchange.db").apply { delete() }
        StayaCore.open(db.path, ByteArray(32) { 5 }).use { core ->
            // Сервер — из приглашения; адрес подключения для эмулятора — свой.
            core.setServerFromLink(invite)
            val client = StayaClient(core, baseUrl = server!!)
            val sync = CoreSync(core, client)
            sync.publishKeys()
            sync.accept(invite)

            // Тот же путь, что у сервиса геопозиции (4.5): замер → ядро → сервер.
            val sender = LocationSender()
            val deadline = System.currentTimeMillis() + TIMEOUT_MS
            var got = false
            var gotAt = 0L
            while (!got || System.currentTimeMillis() - gotAt < LINGER_MS) {
                check(System.currentTimeMillis() < deadline) { "no location from peer" }
                for (event in sync.sync()) {
                    if (event is CoreEvent.LocationUpdated && !got &&
                        event.location.latE7 == PEER_LAT && event.location.lonE7 == PEER_LON
                    ) {
                        got = true
                        gotAt = System.currentTimeMillis()
                    }
                }
                if (core.listFriends().any { it.active }) {
                    sender.onFix(core, sync, Fix(MY_LAT / 1e7, MY_LON / 1e7, 10f, System.currentTimeMillis()))
                }
                Thread.sleep(1000)
            }
            assertTrue(got)
        }
        db.delete()
    }

    private companion object {
        // Должны совпадать с аргументами dev-peer в .github/workflows/ci.yml.
        const val MY_LAT = 599_386_000
        const val MY_LON = 303_141_000
        const val PEER_LAT = 557_558_000
        const val PEER_LON = 376_173_000
        const val TIMEOUT_MS = 5 * 60_000L
        const val LINGER_MS = 20_000L
    }
}
