package io.github.realfamousbae.staya.location

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent

/**
 * Загрузка телефона и обновление приложения: поднять сервис, если пользователь
 * включил передачу. Android 14+ разрешает отсюда тип location только с геолокацией
 * «Разрешить всегда» — иначе запуск не удастся, и это не падение.
 */
class BootReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        if (intent.action != Intent.ACTION_BOOT_COMPLETED && intent.action != Intent.ACTION_MY_PACKAGE_REPLACED) return
        LocationShare.startService(context, fromBackground = true)
    }
}
