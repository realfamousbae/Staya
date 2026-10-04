package io.github.realfamousbae.staya.secure

import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/** Правила docs/protocol.md §3.1: новый ключ — только когда старого точно нет. */
class DbKeyTest {
    private class FakeStore(var stored: ByteArray? = null) : KeyBackend {
        var failRead: SecretRead? = null
        var failAdd = false
        var adds = 0

        override fun read(): SecretRead =
            failRead ?: stored?.let { SecretRead.Found(it.copyOf()) } ?: SecretRead.NotFound

        override fun addIfAbsent(value: ByteArray) {
            adds++
            if (failAdd) throw java.io.IOException("disk full")
            if (stored == null) stored = value.copyOf()
        }
    }

    private class FakeDb(var exists: Boolean) {
        var deletes = 0
        fun delete() {
            deletes++
            exists = false
        }
    }

    private val fresh = ByteArray(32) { 1 }

    private fun obtain(store: FakeStore, db: FakeDb) =
        DbKey.obtain(store, { db.exists }, db::delete) { fresh.copyOf() }

    @Test
    fun firstLaunchCreatesKeyBeforeDb() {
        val store = FakeStore()
        val db = FakeDb(exists = false)
        val r = obtain(store, db)
        assertArrayEquals(fresh, (r as DbKey.Result.Ready).key)
        assertArrayEquals(fresh, store.stored)
        assertEquals(0, db.deletes)
    }

    @Test
    fun existingKeyIsUsedAndNeverReplaced() {
        val old = ByteArray(32) { 9 }
        val store = FakeStore(old)
        val db = FakeDb(exists = true)
        val r = obtain(store, db)
        assertArrayEquals(old, (r as DbKey.Result.Ready).key)
        assertEquals(0, store.adds)
        assertEquals(0, db.deletes)
    }

    @Test
    fun transientErrorNeverCreatesKeyOrTouchesDb() {
        val store = FakeStore().apply { failRead = SecretRead.Unavailable(java.security.KeyStoreException("boot")) }
        val db = FakeDb(exists = true)
        assertTrue(obtain(store, db) is DbKey.Result.Unavailable)
        assertEquals(0, store.adds)
        assertNull(store.stored)
        assertTrue(db.exists)
    }

    @Test
    fun brokenKeyIsReportedNotReplaced() {
        val store = FakeStore().apply { failRead = SecretRead.Broken(javax.crypto.AEADBadTagException()) }
        val db = FakeDb(exists = true)
        assertTrue(obtain(store, db) is DbKey.Result.Broken)
        assertEquals(0, store.adds)
        assertTrue(db.exists)
    }

    @Test
    fun lostKeyDiscardsUnreadableDb() {
        val store = FakeStore()
        val db = FakeDb(exists = true)
        val r = obtain(store, db)
        assertTrue(r is DbKey.Result.Ready)
        assertEquals(1, db.deletes)
        assertFalse(db.exists)
    }

    @Test
    fun failedSaveDoesNotOpen() {
        val store = FakeStore().apply { failAdd = true }
        val db = FakeDb(exists = false)
        assertTrue(obtain(store, db) is DbKey.Result.Unavailable)
        assertNull(store.stored)
    }

    @Test
    fun wrongSizeIsBroken() {
        val store = FakeStore(ByteArray(16))
        assertTrue(obtain(store, FakeDb(exists = true)) is DbKey.Result.Broken)
    }
}
