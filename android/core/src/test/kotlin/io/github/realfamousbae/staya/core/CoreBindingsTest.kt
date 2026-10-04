package io.github.realfamousbae.staya.core

import java.nio.file.Files
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Assert.fail
import org.junit.Test
import uniffi.staya_core.CoreException
import uniffi.staya_core.InviteMethod
import uniffi.staya_core.Location
import uniffi.staya_core.StayaCore
import uniffi.staya_core.coreVersion

/** Проверяет, что Kotlin-привязки реально вызывают Rust-ядро. */
class CoreBindingsTest {
    @Test
    fun coreVersionComesFromRust() {
        assertEquals("0.1.0", coreVersion())
    }

    @Test
    fun coreApiWorksThroughBindings() {
        val path = Files.createTempDirectory("staya").resolve("staya.db").toString()
        val key = ByteArray(32) { 7 }
        StayaCore.open(path, key).use { core ->
            val me = core.identity()
            assertEquals(22, me.accountId.length)

            core.setServer("staya.test", emptyList())
            val invite = core.createInvite(InviteMethod.QR, 1_700_000_000)
            assertTrue(invite.startsWith("staya://add?"))
            assertEquals(me.accountId, core.parseInvite(invite).accountId)
            assertEquals("staya.test", core.parseInvite(invite).server)

            assertTrue(core.keysToPublish(0u, 1_700_000_000)!!.contains("one_time_keys"))
            assertTrue(core.listFriends().isEmpty())

            core.setFrozen(Location(557_558_000, 376_173_000, 10u.toUShort(), 1_700_000_000))
            assertTrue(core.sharing().frozen != null)
            assertFalse(core.sharing().ghost)
        }
    }

    @Test
    fun coreErrorsBecomeKotlinExceptions() {
        val path = Files.createTempDirectory("staya").resolve("staya.db").toString()
        StayaCore.open(path, ByteArray(32)).use { core ->
            try {
                core.parseInvite("https://example.com")
                fail("expected an exception")
            } catch (e: CoreException) {
                // Ошибка ядра пришла как исключение с текстом.
            }
        }
    }
}
