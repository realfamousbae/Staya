package io.github.realfamousbae.staya.location

import io.github.realfamousbae.staya.net.CoreSync
import kotlin.math.asin
import kotlin.math.ceil
import kotlin.math.cos
import kotlin.math.roundToLong
import kotlin.math.sin
import kotlin.math.sqrt
import uniffi.staya_core.Location
import uniffi.staya_core.StayaCore

/**
 * Замер с устройства без типов Android (проверяется JVM-тестами). Никогда не
 * печатать: `toString` скрывает координаты.
 */
class Fix(val latitude: Double, val longitude: Double, val accuracyM: Float?, val timeMs: Long) {
    override fun toString() = "Fix(<redacted>, t=$timeMs)"
}

/** Правила отправки позиции (задача 4.5, сторож — 5.1), без Android и сети. */
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

    /**
     * Сторож (5.1): будильник раз в столько минут поднимает сервис, если его убили,
     * и просит один свежий замер — на случай, если поток замеров в покое иссяк.
     * `setAndAllowWhileIdle` в Doze всё равно срабатывает не чаще раза в ~9 минут.
     */
    const val WATCHDOG_INTERVAL_MS = 15 * 60_000L

    /**
     * В покое сдвиг больше этого (сверх погрешностей обоих замеров) от первого замера
     * покоя — телефон поехал, а датчик значимого движения промолчал (его нет или он не
     * сработал): точный режим. Погрешности учитываются, иначе точки уровня соты
     * (±500–2000 м без Wi-Fi) дёргали бы GPS на каждом дрожании.
     */
    const val MOVED_M = 250.0

    /** Свежий одиночный замер по запросу — не чаще: иначе покой превратился бы в опрос GPS. */
    const val MIN_REFRESH_INTERVAL_MS = 5 * 60_000L

    fun shouldRefresh(nowMs: Long, lastRefreshMs: Long?): Boolean =
        lastRefreshMs == null || nowMs - lastRefreshMs >= MIN_REFRESH_INTERVAL_MS || nowMs < lastRefreshMs

    private const val EARTH_RADIUS_M = 6_371_008.8

    /** Расстояние по большому кругу (гаверсинус), метры. */
    fun distanceM(lat1: Double, lon1: Double, lat2: Double, lon2: Double): Double {
        val p1 = Math.toRadians(lat1)
        val p2 = Math.toRadians(lat2)
        val dp = Math.toRadians(lat2 - lat1)
        val dl = Math.toRadians(lon2 - lon1)
        val a = sin(dp / 2) * sin(dp / 2) + cos(p1) * cos(p2) * sin(dl / 2) * sin(dl / 2)
        return 2 * EARTH_RADIUS_M * asin(sqrt(a))
    }

    /** Замер без точности — не якорь и не сдвиг. */
    fun moved(from: Fix, to: Fix): Boolean {
        val a = from.accuracyM ?: return false
        val b = to.accuracyM ?: return false
        return distanceM(from.latitude, from.longitude, to.latitude, to.longitude) > MOVED_M + a + b
    }
}

/**
 * Ограничение частоты с придержанной точкой (как `SendThrottle` на iOS): отброшенный
 * по частоте замер запоминается и уходит, когда движение остановилось (машина
 * припарковалась) — иначе на карте осталась бы точка до минуты езды назад.
 */
class SendThrottle(private val clock: () -> Long = System::currentTimeMillis) {
    private var lastSentMs: Long? = null
    private var held: Fix? = null

    /** Замер, который надо отправить сейчас, или `null` (придержан). */
    fun offer(fix: Fix): Fix? {
        if (!LocationPolicy.shouldSend(clock(), lastSentMs)) {
            held = fix
            return null
        }
        lastSentMs = clock()
        held = null
        return fix
    }

    /** Придержанный замер — без ограничения частоты. */
    fun takeHeld(): Fix? {
        val fix = held ?: return null
        held = null
        lastSentMs = clock()
        return fix
    }

    fun reset() {
        lastSentMs = null
        held = null
    }
}

/**
 * Замер → ядро → сервер. Только из [io.github.realfamousbae.staya.net.Net.worker].
 * Сначала пакет надёжно ложится в исходящую очередь ядра, потом — попытка
 * отправки; не ушедшее уйдёт при следующей отправке или открытии приложения.
 */
class LocationSender(private val clock: () -> Long = System::currentTimeMillis) {
    private val throttle = SendThrottle(clock)

    /** Последняя неудача отправки (класс исключения, без адресов и координат); `null` — ушло. */
    var lastSendError: String? = null
        private set

    /** Сброс ограничения частоты (передачу только что включили). */
    fun reset() {
        throttle.reset()
    }

    /** `true` — пакеты поставлены в очередь. Негодный или старый замер не придерживается. */
    fun onFix(core: StayaCore, sync: CoreSync?, fix: Fix): Boolean {
        if (LocationPolicy.toCore(fix) == null || !LocationPolicy.isFresh(fix, clock())) return false
        val send = throttle.offer(fix) ?: return false
        queue(core, sync, send)
        return true
    }

    /** Движение остановилось: придержанный замер уходит сразу. `true` — было что отправить. */
    fun flushHeld(core: StayaCore, sync: CoreSync?): Boolean {
        val fix = throttle.takeHeld() ?: return false
        queue(core, sync, fix)
        return true
    }

    private fun queue(core: StayaCore, sync: CoreSync?, fix: Fix) {
        val location = LocationPolicy.toCore(fix) ?: return
        core.prepareLocationUpdate(location, clock() / 1000)
        lastSendError = runCatching { sync?.flush() }.exceptionOrNull()?.javaClass?.simpleName
    }
}
