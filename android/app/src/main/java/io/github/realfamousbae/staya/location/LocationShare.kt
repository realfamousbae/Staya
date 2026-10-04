package io.github.realfamousbae.staya.location

import android.Manifest
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import androidx.core.content.edit
import io.github.realfamousbae.staya.AppCore
import io.github.realfamousbae.staya.net.Net
import uniffi.staya_core.StayaCore

/**
 * «Делиться позицией» (задача 4.5): явное согласие пользователя, сервис геопозиции
 * и путь замер → ядро → сервер. Флаг — не секрет (только «включено ли»), поэтому
 * в SharedPreferences. Выключение — режим призрака: друзья получают «скрыто», а не
 * видят последнюю точку как текущую.
 */
object LocationShare {
    private const val PREFS = "location"
    private const val KEY_ENABLED = "sharing"

    private val sender = LocationSender()

    fun isEnabled(context: Context): Boolean =
        context.getSharedPreferences(PREFS, Context.MODE_PRIVATE).getBoolean(KEY_ENABLED, false)

    private fun setEnabled(context: Context, on: Boolean) {
        context.getSharedPreferences(PREFS, Context.MODE_PRIVATE).edit { putBoolean(KEY_ENABLED, on) }
    }

    fun hasForegroundPermission(context: Context) =
        granted(context, Manifest.permission.ACCESS_FINE_LOCATION) ||
            granted(context, Manifest.permission.ACCESS_COARSE_LOCATION)

    fun hasBackgroundPermission(context: Context) = granted(context, Manifest.permission.ACCESS_BACKGROUND_LOCATION)

    fun granted(context: Context, permission: String) =
        context.checkSelfPermission(permission) == PackageManager.PERMISSION_GRANTED

    /** Явное «Делиться позицией» на экране (разрешение на геолокацию уже есть). */
    fun enable(context: Context, core: StayaCore) {
        val app = context.applicationContext
        setEnabled(app, true)
        Net.worker.execute {
            // Первый замер после включения уходит сразу, а не через минуту «скрыто».
            sender.reset()
            runCatching { core.setGhost(false) }.onSuccess { resend(core) }
        }
        startService(app, fromBackground = false)
    }

    /** Выключение: сервис стоп, друзьям — «скрыто». */
    fun disable(context: Context, core: StayaCore) {
        val app = context.applicationContext
        setEnabled(app, false)
        app.stopService(Intent(app, LocationService::class.java))
        Net.worker.execute {
            runCatching {
                core.setGhost(true)
                core.prepareLocationUpdate(null, System.currentTimeMillis() / 1000)
                Net.bind(core)?.second?.flush()
            }
        }
    }

    /** «Заморозить здесь»: друзья видят последнюю точку, пока заморозку не снимут. */
    fun setFrozen(core: StayaCore, frozen: Boolean, onResult: (Boolean) -> Unit) {
        Net.worker.execute {
            val ok = runCatching { if (frozen) core.freezeHere() else core.setFrozen(null) }.isSuccess
            if (ok) resend(core)
            onResult(ok)
        }
    }

    fun isFrozen(core: StayaCore): Boolean = runCatching { core.sharing().frozen != null }.getOrDefault(false)

    /**
     * Пакеты всем друзьям из последнего замера (смена режима или точности, 4.6).
     * Замера ещё не было — отправлять нечего, это не ошибка. Только из [Net.worker].
     */
    fun resend(core: StayaCore) {
        val queued = runCatching { core.prepareLocationUpdate(null, System.currentTimeMillis() / 1000) }
        if (queued.isSuccess) runCatching { Net.bind(core)?.second?.flush() }
    }

    /**
     * Поднять сервис, если пользователь включил передачу. Из фона (загрузка,
     * обновление) Android 14+ разрешает тип location только с «Разрешить всегда»:
     * без неё не запускаем вовсе — иначе сервис не смог бы войти в передний план.
     */
    fun startService(context: Context, fromBackground: Boolean): Boolean {
        if (!isEnabled(context) || !hasForegroundPermission(context)) return false
        if (fromBackground && !hasBackgroundPermission(context)) return false
        return runCatching {
            context.startForegroundService(Intent(context, LocationService::class.java))
        }.isSuccess
    }

    /** Новый замер из сервиса. Ядро открывается и без экрана; если база ещё недоступна — пропуск. */
    fun onFix(context: Context, fix: Fix) {
        val app = context.applicationContext
        Net.worker.execute {
            if (!isEnabled(app)) return@execute
            val core = AppCore.openBlocking(app) ?: return@execute
            runCatching { sender.onFix(core, Net.bind(core)?.second, fix) }
        }
    }
}
