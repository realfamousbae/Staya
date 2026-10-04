package io.github.realfamousbae.staya.net

import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import java.io.File
import org.junit.Assert.assertTrue
import org.junit.Assume.assumeTrue
import org.junit.Test
import org.junit.runner.RunWith
import uniffi.staya_core.CoreEvent
import uniffi.staya_core.Location
import uniffi.staya_core.StayaCore

/**
 * Обмен позициями с тестовым собеседником (`tools/dev-peer`) через dev-сервер на
 * хосте (задача 2.10). Запускается в CI с аргументом `devServer` (обычно
 * `http://10.0.2.2:8787`); без него пропускается. База — временная, не база приложения.
 */
@RunWith(AndroidJUnit4::class)
class DevExchangeTest {
    @Test
    fun exchangesLocationsWithPeer() {
        val server = InstrumentationRegistry.getArguments().getString("devServer")
        assumeTrue("no devServer argument", server != null)
        val context = InstrumentationRegistry.getInstrumentation().targetContext
        val db = File(context.cacheDir, "dev-exchange.db").apply { delete() }
        StayaCore.open(db.path, ByteArray(32) { 5 }).use { core ->
            val me = core.identity().accountId
            val http = StayaHttp(server!!) { me }
            val sync = CoreSync(core, http)
            sync.publishKeys()

            val deadline = System.currentTimeMillis() + TIMEOUT_MS
            var invite: String? = null
            while (invite == null) {
                invite = runCatching { http.getPublic("/dev/invite") }.getOrNull()
                if (invite == null) {
                    check(System.currentTimeMillis() < deadline) { "no invite from peer" }
                    Thread.sleep(500)
                }
            }
            sync.accept(invite)

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
                    sync.share(Location(MY_LAT, MY_LON, 10u.toUShort(), System.currentTimeMillis() / 1000))
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
