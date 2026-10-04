package io.github.realfamousbae.staya.secure

import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import java.io.File
import org.junit.After
import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import uniffi.staya_core.StayaCore

/** Настоящий Android Keystore и ядро на эмуляторе (CI). */
@RunWith(AndroidJUnit4::class)
class DbKeyDeviceTest {
    private val context = InstrumentationRegistry.getInstrumentation().targetContext
    private val store = SecretStore(context, "test-db-key")
    private val db = File(context.cacheDir, "test-staya.db")

    @After
    fun cleanup() {
        store.delete()
        db.delete()
        File(db.path + "-journal").delete()
    }

    @Test
    fun keystoreRoundTripAndAddOnly() {
        store.delete()
        assertEquals(SecretRead.NotFound, store.read())
        val first = ByteArray(32) { 3 }
        store.addIfAbsent(first)
        assertArrayEquals(first, (store.read() as SecretRead.Found).value)
        store.addIfAbsent(ByteArray(32) { 4 })
        assertArrayEquals(first, (store.read() as SecretRead.Found).value)
        store.delete()
        assertEquals(SecretRead.NotFound, store.read())
    }

    @Test
    fun coreReopensWithStoredKey() {
        store.delete()
        fun key() = (DbKey.obtain(store, db::exists, { db.delete() }) as DbKey.Result.Ready).key
        val id = StayaCore.open(db.path, key()).use { it.identity().accountId }
        assertTrue(db.exists())
        val again = StayaCore.open(db.path, key()).use { it.identity().accountId }
        assertEquals(id, again)
    }
}
