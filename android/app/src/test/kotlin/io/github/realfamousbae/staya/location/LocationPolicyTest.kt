package io.github.realfamousbae.staya.location

import java.io.File
import java.nio.file.Files
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import uniffi.staya_core.StayaCore

class LocationPolicyTest {
    private val t = 1_700_000_000_000L

    @Test
    fun convertsFixToCoreLocation() {
        val loc = LocationPolicy.toCore(Fix(55.7558, 37.6173, 9.2f, t + 999))!!
        assertEquals(557_558_000, loc.latE7)
        assertEquals(376_173_000, loc.lonE7)
        assertEquals(10, loc.accuracyM.toInt()) // вверх: точность не приукрашиваем
        assertEquals(t / 1000, loc.timestamp) // время замера, не отправки
        assertEquals(-338_000_000, LocationPolicy.toCore(Fix(-33.8, -70.0, 5f, t))!!.latE7)
    }

    @Test
    fun rejectsBadFixesAndClampsAccuracy() {
        assertNull(LocationPolicy.toCore(Fix(55.0, 37.0, null, t)))
        assertNull(LocationPolicy.toCore(Fix(55.0, 37.0, -1f, t)))
        assertNull(LocationPolicy.toCore(Fix(55.0, 37.0, Float.NaN, t)))
        assertNull(LocationPolicy.toCore(Fix(91.0, 37.0, 5f, t)))
        assertNull(LocationPolicy.toCore(Fix(55.0, 181.0, 5f, t)))
        assertNull(LocationPolicy.toCore(Fix(Double.NaN, 37.0, 5f, t)))
        assertEquals(65_535, LocationPolicy.toCore(Fix(55.0, 37.0, 1e9f, t))!!.accuracyM.toInt())
    }

    @Test
    fun fixDoesNotPrintCoordinates() {
        assertFalse(Fix(55.7558, 37.6173, 5f, t).toString().contains("55.7"))
    }

    @Test
    fun staleFixesAreNotSent() {
        val fix = Fix(55.0, 37.0, 5f, t)
        assertTrue(LocationPolicy.isFresh(fix, t + 600_000))
        assertFalse(LocationPolicy.isFresh(fix, t + 600_001))
        assertFalse(LocationPolicy.isFresh(fix, t + 35 * 3_600_000L))
        assertTrue(LocationPolicy.isFresh(fix, t - 120_000)) // часы спешат
    }

    @Test
    fun throttlesToOncePerMinute() {
        assertTrue(LocationPolicy.shouldSend(t, null))
        assertFalse(LocationPolicy.shouldSend(t + 59_999, t))
        assertTrue(LocationPolicy.shouldSend(t + 60_000, t))
        assertTrue(LocationPolicy.shouldSend(t - 1, t)) // часы перевели назад
    }

    @Test
    fun mailboxIsPolledAtMostEveryFiveMinutes() {
        val t = 1_700_000_000_000L
        assertTrue(LocationPolicy.shouldPollMailbox(t, null))
        assertFalse(LocationPolicy.shouldPollMailbox(t + 299_000, t))
        assertTrue(LocationPolicy.shouldPollMailbox(t + 300_000, t))
        assertTrue("clock moved back", LocationPolicy.shouldPollMailbox(t - 10_000, t))
    }

    @Test
    fun heldFixGoesOutWhenMovementStops() {
        var now = t
        val throttle = SendThrottle { now }
        val first = Fix(55.0, 37.0, 5f, now)
        assertTrue(throttle.offer(first) === first)
        now += 20_000
        val second = Fix(55.001, 37.0, 5f, now)
        assertNull(throttle.offer(second)) // придержан
        now += 10_000
        val third = Fix(55.002, 37.0, 5f, now)
        assertNull(throttle.offer(third)) // придерживается только последний
        assertTrue(throttle.takeHeld() === third)
        assertNull(throttle.takeHeld()) // второй раз нечего
        now += 1_000
        assertNull(throttle.offer(Fix(55.003, 37.0, 5f, now))) // отправка придержанного считается отправкой
    }

    @Test
    fun movementInRestIsNoticedWithoutMotionSensor() {
        val home = Fix(55.7558, 37.6173, 50f, t)
        assertFalse(LocationPolicy.moved(home, Fix(55.7568, 37.6173, 50f, t))) // ~110 м — шум сети
        assertTrue(LocationPolicy.moved(home, Fix(55.7558, 37.6253, 50f, t))) // ~500 м — поехали
        assertEquals(0.0, LocationPolicy.distanceM(55.0, 37.0, 55.0, 37.0), 1e-9)
        // Москва — Санкт-Петербург ≈ 634 км.
        assertEquals(634_000.0, LocationPolicy.distanceM(55.7558, 37.6173, 59.9343, 30.3351), 2_000.0)
    }

    @Test
    fun refreshIsRateLimited() {
        assertTrue(LocationPolicy.shouldRefresh(t, null))
        assertFalse(LocationPolicy.shouldRefresh(t + 299_000, t))
        assertTrue(LocationPolicy.shouldRefresh(t + 300_000, t))
        assertTrue(LocationPolicy.shouldRefresh(t - 1, t))
    }

    @Test
    fun senderFlushesHeldFixIntoTheRealCore() {
        val dir = Files.createTempDirectory("staya-loc").toFile()
        try {
            StayaCore.open(File(dir, "db").path, ByteArray(32) { 3 }).use { core ->
                var now = t
                val sender = LocationSender { now }
                assertFalse(sender.flushHeld(core, null)) // ничего не придержано
                assertTrue(sender.onFix(core, null, Fix(55.0, 37.0, 5f, now)))
                now += 10_000
                assertFalse(sender.onFix(core, null, Fix(55.001, 37.0, 5f, now)))
                assertTrue(sender.flushHeld(core, null))
                assertFalse(sender.flushHeld(core, null))
                assertNull(sender.lastSendError)
            }
        } finally {
            dir.deleteRecursively()
        }
    }

    @Test
    fun senderQueuesIntoTheRealCoreAndThrottles() {
        val dir = Files.createTempDirectory("staya-loc").toFile()
        try {
            StayaCore.open(File(dir, "db").path, ByteArray(32) { 3 }).use { core ->
                var now = t
                val sender = LocationSender { now }
                assertFalse(sender.onFix(core, null, Fix(55.0, 37.0, null, now))) // негодный замер
                assertFalse(sender.onFix(core, null, Fix(55.0, 37.0, 5f, now - 3_600_000))) // старая точка
                assertTrue(sender.onFix(core, null, Fix(55.0, 37.0, 5f, now)))
                now += 10_000
                assertFalse(sender.onFix(core, null, Fix(55.0, 37.0, 5f, now)))
                now += 60_000
                assertTrue(sender.onFix(core, null, Fix(55.0, 37.0, 5f, now)))
                // Друзей нет — отправлять нечего, но и ошибки нет.
                assertNotNull(core.pendingSends())
            }
        } finally {
            dir.deleteRecursively()
        }
    }
}
