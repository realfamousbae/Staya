package io.github.realfamousbae.staya.probe

/**
 * Метрика для сборщика этапа 1 (tools/probe-server, docs/server.md).
 *
 * Имена полей и значения должны совпадать со схемой сервера — он отвергает всё
 * лишнее. Координат здесь нет и быть не должно: только точность и корзина скорости.
 * Чистый Kotlin без Android, чтобы проверять кодирование на JVM.
 */
data class ProbeRecord(
    val device: String,
    val strategy: String,
    /** Unix-время события, секунды. */
    val eventTs: Long,
    val trigger: Trigger,
    val appState: AppState,
    /** Точность замера в метрах; `null`, если замера нет. */
    val accuracyM: Long?,
    val speed: Speed,
    val batteryPct: Int,
    val charging: Boolean,
    val lowPower: Boolean,
    val prevSendMs: Long?,
    val prevSendFailures: Long,
    val auth: Auth,
    val precise: Boolean,
    val eventsSinceLast: Long,
    val batteryOptExempt: Boolean?,
    val bgRestricted: Boolean?,
    val standbyBucket: StandbyBucket?,
    val doze: Boolean?,
    val provider: Provider?,
    val sigMotion: Boolean?,
) {
    enum class Trigger(val wire: String) {
        SIGNIFICANT_CHANGE("significant_change"),
        VISIT("visit"),
        CONTINUOUS("continuous"),
        TIMER("timer"),
        MOTION("motion"),
        FOREGROUND("foreground"),
        BOOT("boot"),
        CONTINUOUS_START("continuous_start"),
        CONTINUOUS_STOP("continuous_stop"),
    }

    enum class AppState(val wire: String) { FOREGROUND("foreground"), BACKGROUND("background"), RELAUNCHED("relaunched") }

    enum class Speed(val wire: String) {
        UNKNOWN("unknown"), STILL("still"), WALKING("walking"), DRIVING("driving");

        companion object {
            /** Метры в секунду; отрицательное — неизвестно. */
            fun of(metersPerSecond: Float): Speed = when {
                metersPerSecond < 0f -> UNKNOWN
                metersPerSecond < 0.5f -> STILL
                metersPerSecond < 3f -> WALKING
                else -> DRIVING
            }
        }
    }

    enum class Auth(val wire: String) {
        ALWAYS("always"), WHEN_IN_USE("when_in_use"), DENIED("denied"), RESTRICTED("restricted"), NOT_DETERMINED("not_determined")
    }

    enum class StandbyBucket(val wire: String) {
        EXEMPTED("exempted"), ACTIVE("active"), WORKING_SET("working_set"), FREQUENT("frequent"),
        RARE("rare"), RESTRICTED("restricted"), UNKNOWN("unknown")
    }

    enum class Provider(val wire: String) { FUSED("fused"), GPS("gps"), NETWORK("network"), PASSIVE("passive") }

    /** JSON со всеми полями; необязательные — явный `null`, как ждёт сервер. */
    fun toJson(): String = buildString {
        append('{')
        field("device", str(device)); field("platform", str("android")); field("strategy", str(strategy))
        field("event_ts", eventTs.toString()); field("trigger", str(trigger.wire)); field("app_state", str(appState.wire))
        field("accuracy_m", accuracyM?.toString() ?: "null"); field("speed", str(speed.wire))
        field("battery_pct", batteryPct.coerceIn(0, 100).toString()); field("charging", charging.toString())
        field("low_power", lowPower.toString()); field("prev_send_ms", prevSendMs?.toString() ?: "null")
        field("prev_send_failures", prevSendFailures.toString()); field("auth", str(auth.wire))
        field("precise", precise.toString()); field("bg_refresh", "null")
        field("events_since_last", eventsSinceLast.toString())
        field("battery_opt_exempt", batteryOptExempt?.toString() ?: "null")
        field("bg_restricted", bgRestricted?.toString() ?: "null")
        field("standby_bucket", standbyBucket?.let { str(it.wire) } ?: "null")
        field("doze", doze?.toString() ?: "null")
        field("provider", provider?.let { str(it.wire) } ?: "null")
        field("sig_motion", sigMotion?.toString() ?: "null", last = true)
        append('}')
    }

    private fun StringBuilder.field(name: String, value: String, last: Boolean = false) {
        append('"').append(name).append("\":").append(value)
        if (!last) append(',')
    }

    companion object {
        /** Строка JSON с экранированием по RFC 8259. */
        fun str(s: String): String = buildString {
            append('"')
            for (c in s) {
                when {
                    c == '"' -> append("\\\"")
                    c == '\\' -> append("\\\\")
                    c == '\n' -> append("\\n")
                    c == '\r' -> append("\\r")
                    c == '\t' -> append("\\t")
                    c < ' ' -> append("\\u%04x".format(c.code))
                    else -> append(c)
                }
            }
            append('"')
        }

        /** Точность Android в метрах, округлённая; отрицательная или NaN — нет данных. */
        fun accuracy(meters: Float?): Long? =
            meters?.takeIf { it >= 0f && !it.isNaN() && !it.isInfinite() }?.let { Math.round(it.toDouble()) }
    }
}
