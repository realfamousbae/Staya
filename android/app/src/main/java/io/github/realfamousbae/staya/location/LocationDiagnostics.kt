package io.github.realfamousbae.staya.location

import android.content.Context
import androidx.core.content.edit

/**
 * Что происходило с передачей — без координат: только времена и причины (задача 5.1).
 * Нужно, чтобы по карточке «Делиться позицией» отличить «телефон лежал» от «сервис
 * умер» или «пакеты не уходят». Флаги не секрет — SharedPreferences.
 */
object LocationDiagnostics {
    private const val PREFS = "location-diagnostics"

    const val FIX_AT = "fix_at"
    const val SENT_AT = "sent_at"
    const val SEND_ERROR_AT = "send_error_at"
    const val SEND_ERROR = "send_error"
    const val SERVICE_FAILED_AT = "service_failed_at"
    const val SERVICE_FAILED = "service_failed"
    const val WATCHDOG_AT = "watchdog_at"
    const val DROP_AT = "drop_at"
    const val DROP = "drop"

    class Snapshot(
        val fixAt: Long?,
        val sentAt: Long?,
        val sendError: String?,
        val sendErrorAt: Long?,
        val serviceFailed: String?,
        val serviceFailedAt: Long?,
        val watchdogAt: Long?,
        val drop: String?,
        val dropAt: Long?,
    )

    private fun prefs(context: Context) = context.applicationContext.getSharedPreferences(PREFS, Context.MODE_PRIVATE)

    /** Отметка времени «сейчас» по ключу. */
    fun mark(context: Context, key: String) {
        prefs(context).edit { putLong(key, System.currentTimeMillis()) }
    }

    /** Причина (короткая, без секретов) и время. */
    fun reason(context: Context, key: String, atKey: String, reason: String) {
        prefs(context).edit {
            putString(key, reason.take(80))
            putLong(atKey, System.currentTimeMillis())
        }
    }

    fun clear(context: Context, vararg keys: String) {
        prefs(context).edit { keys.forEach { remove(it) } }
    }

    fun snapshot(context: Context): Snapshot {
        val p = prefs(context)
        fun at(key: String) = p.getLong(key, 0L).takeIf { it > 0 }
        return Snapshot(
            fixAt = at(FIX_AT),
            sentAt = at(SENT_AT),
            sendError = p.getString(SEND_ERROR, null),
            sendErrorAt = at(SEND_ERROR_AT),
            serviceFailed = p.getString(SERVICE_FAILED, null),
            serviceFailedAt = at(SERVICE_FAILED_AT),
            watchdogAt = at(WATCHDOG_AT),
            drop = p.getString(DROP, null),
            dropAt = at(DROP_AT),
        )
    }
}
