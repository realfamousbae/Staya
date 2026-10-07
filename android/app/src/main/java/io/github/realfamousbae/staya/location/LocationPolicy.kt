package io.github.realfamousbae.staya.location

import io.github.realfamousbae.staya.net.CoreSync
import kotlin.math.ceil
import kotlin.math.roundToLong
import uniffi.staya_core.Location
import uniffi.staya_core.StayaCore

/**
 * Замер с устройства без типов Android (проверяется JVM-тестами). Никогда не
 * печатать: `toString` скрывает координаты.
 */
class Fix(val latitude: Double, val longitude: Double, val accuracyM: Float?, val timeMs: Long) {
    override fun toString() = "Fix(<redacted>, t=$timeMs)"
}

/** Правила отправки позиции (задача 4.5), без Android и сети. */
object LocationPolicy {
    /** Не чаще: в машине при шаге 100 м точки идут каждые несколько секунд. */
    const val MIN_SEND_INTERVAL_MS = 60_000L

    /**
     * Замер → позиция ядра (градусы × 10⁷). `null` — замер негоден: координаты вне
     * диапазона, нет или отрицательная точность. Время — время самого замера.
     */
    fun toCore(fix: Fix): Location? {
        val acc = fix.accuracyM ?: return null
        if (acc.isNaN() || acc < 0f) return null
        if (fix.latitude.isNaN() || fix.longitude.isNaN()) return null
        if (fix.latitude !in -90.0..90.0 || fix.longitude !in -180.0..180.0) return null
        val accuracy = ceil(acc.toDouble()).coerceAtMost(UShort.MAX_VALUE.toDouble()).toInt()
        return Location(
            (fix.latitude * 1e7).roundToLong().toInt(),
            (fix.longitude * 1e7).roundToLong().toInt(),
            accuracy.toUShort(),
            fix.timeMs / 1000,
        )
    }

    /** Старше — не отправляем: система может отдать давно сохранённую точку. */
    const val MAX_FIX_AGE_MS = 10 * 60_000L

    fun isFresh(fix: Fix, nowMs: Long): Boolean = nowMs - fix.timeMs <= MAX_FIX_AGE_MS

    fun shouldSend(nowMs: Long, lastSentMs: Long?): Boolean =
        lastSentMs == null || nowMs - lastSentMs >= MIN_SEND_INTERVAL_MS || nowMs < lastSentMs

    /**
     * Ящик в фоне (4.9d) — не чаще: заявка в друзья по ссылке ждёт ответа сутки
     * (protocol §5), минуты задержки не важны, а лишние запросы тратят батарею.
     */
    const val MIN_MAILBOX_INTERVAL_MS = 5 * 60_000L

    fun shouldPollMailbox(nowMs: Long, lastPollMs: Long?): Boolean =
        lastPollMs == null || nowMs - lastPollMs >= MIN_MAILBOX_INTERVAL_MS || nowMs < lastPollMs
}

/**
 * Замер → ядро → сервер. Только из [io.github.realfamousbae.staya.net.Net.worker].
 * Сначала пакет надёжно ложится в исходящую очередь ядра, потом — попытка
 * отправки; не ушедшее уйдёт при следующей отправке или открытии приложения.
 */
class LocationSender(private val clock: () -> Long = System::currentTimeMillis) {
    private var lastSentMs: Long? = null

    /** Сброс ограничения частоты (передачу только что включили). */
    fun reset() {
        lastSentMs = null
    }

    /** `true` — пакеты поставлены в очередь. */
    fun onFix(core: StayaCore, sync: CoreSync?, fix: Fix): Boolean {
        val location = LocationPolicy.toCore(fix) ?: return false
        val now = clock()
        if (!LocationPolicy.isFresh(fix, now)) return false
        if (!LocationPolicy.shouldSend(now, lastSentMs)) return false
        core.prepareLocationUpdate(location, now / 1000)
        lastSentMs = now
        runCatching { sync?.flush() }
        return true
    }
}
