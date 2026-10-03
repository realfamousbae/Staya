package io.github.realfamousbae.staya.probe

import org.junit.Assert.assertEquals
import org.junit.Assume.assumeTrue
import org.junit.Test

/**
 * Контракт ProbeRecord ↔ tools/probe-server: каждое значение каждого перечисления и
 * `null` должны давать 204. Запускается из scripts/test-android-probe.sh, который
 * поднимает локальный сервер и передаёт PROBE_URL и PROBE_TOKEN; без них пропускается.
 */
class ProbeContractTest {
    @Test
    fun everyVariantIsAccepted() {
        val url = System.getenv("PROBE_URL")
        val token = System.getenv("PROBE_TOKEN")
        assumeTrue("needs a local probe-server (scripts/test-android-probe.sh)", url != null && token != null)

        val base = ProbeRecordTest.sample()
        val cases = buildList {
            add("full" to base)
            add(
                "nulls" to base.copy(
                    accuracyM = null, prevSendMs = null, batteryOptExempt = null, bgRestricted = null,
                    standbyBucket = null, doze = null, provider = null, sigMotion = null,
                ),
            )
            ProbeRecord.Trigger.entries.forEach { add("trigger ${it.wire}" to base.copy(trigger = it)) }
            ProbeRecord.AppState.entries.forEach { add("state ${it.wire}" to base.copy(appState = it)) }
            ProbeRecord.Speed.entries.forEach { add("speed ${it.wire}" to base.copy(speed = it)) }
            ProbeRecord.Auth.entries.forEach { add("auth ${it.wire}" to base.copy(auth = it)) }
            ProbeRecord.StandbyBucket.entries.forEach { add("bucket ${it.wire}" to base.copy(standbyBucket = it)) }
            ProbeRecord.Provider.entries.forEach { add("provider ${it.wire}" to base.copy(provider = it)) }
            add("escaped device" to base.copy(device = "dev-\"q\""))
        }
        val failed = cases.filter { (_, r) -> ProbeClient.send(r.toJson(), token!!, url!!).status != 204 }.map { it.first }
        assertEquals("rejected: $failed", emptyList<String>(), failed)
        println("contract: ${cases.size}/${cases.size} accepted")
    }
}
