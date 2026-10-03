package io.github.realfamousbae.staya.probe

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class ProbeRecordTest {
    @Test
    fun encodesAllFieldsWithExplicitNulls() {
        val json = sample().copy(accuracyM = null, prevSendMs = null, standbyBucket = null, provider = null).toJson()
        for (key in listOf("accuracy_m", "prev_send_ms", "standby_bucket", "provider", "bg_refresh")) {
            assertTrue("$key must be null: $json", json.contains("\"$key\":null"))
        }
        assertTrue(json.contains("\"platform\":\"android\""))
        assertTrue(json.contains("\"trigger\":\"continuous_start\""))
        assertTrue(json.startsWith("{") && json.endsWith("}"))
        assertFalse(json.contains(",}"))
    }

    @Test
    fun escapesStrings() {
        assertEquals("\"a\\\"b\\\\c\\n\\t\\u0001\"", ProbeRecord.str("a\"b\\c\n\t\u0001"))
        // Строка JSON без переводов строки — очередь хранит записи построчно.
        assertFalse(sample().copy(device = "x\ny").toJson().contains("\n"))
    }

    @Test
    fun accuracyAndSpeedEdgeCases() {
        assertEquals(null, ProbeRecord.accuracy(-1f))
        assertEquals(null, ProbeRecord.accuracy(Float.NaN))
        assertEquals(66L, ProbeRecord.accuracy(65.6f))
        assertEquals(ProbeRecord.Speed.UNKNOWN, ProbeRecord.Speed.of(-1f))
        assertEquals(ProbeRecord.Speed.STILL, ProbeRecord.Speed.of(0.2f))
        assertEquals(ProbeRecord.Speed.WALKING, ProbeRecord.Speed.of(1.4f))
        assertEquals(ProbeRecord.Speed.DRIVING, ProbeRecord.Speed.of(20f))
    }

    @Test
    fun clampsBattery() {
        assertTrue(sample().copy(batteryPct = 150).toJson().contains("\"battery_pct\":100"))
    }

    companion object {
        fun sample() = ProbeRecord(
            device = "contract", strategy = "a3", eventTs = 1_700_000_000, trigger = ProbeRecord.Trigger.CONTINUOUS_START,
            appState = ProbeRecord.AppState.RELAUNCHED, accuracyM = 12, speed = ProbeRecord.Speed.DRIVING,
            batteryPct = 64, charging = false, lowPower = true, prevSendMs = 300, prevSendFailures = 2,
            auth = ProbeRecord.Auth.ALWAYS, precise = true, eventsSinceLast = 3, batteryOptExempt = true,
            bgRestricted = false, standbyBucket = ProbeRecord.StandbyBucket.ACTIVE, doze = false,
            provider = ProbeRecord.Provider.FUSED, sigMotion = true,
        )
    }
}
