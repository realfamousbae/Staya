package io.github.realfamousbae.staya.location

import android.app.AlarmManager
import android.app.PendingIntent
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.os.SystemClock

/**
 * Сторож передачи (задача 5.1). Сервис геопозиции убивают производители телефонов и
 * сама система, а перезапуск «липкого» сервиса из фона Android 12+ запрещает — до
 * этого сервис молча умирал до следующего открытия приложения, и друзья видели
 * точку многодневной давности. Будильник раз в [LocationPolicy.WATCHDOG_INTERVAL_MS]
 * поднимает сервис заново (разрешено, пока приложение исключено из оптимизации
 * батареи — это и просит карточка «Делиться позицией») и просит свежий замер.
 *
 * Один одноразовый будильник, взводится заново при каждом срабатывании: повторяющиеся
 * будильники в Doze не срабатывают, а `setAndAllowWhileIdle` — в окна обслуживания.
 */
class Watchdog : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        if (intent.action != ACTION || !LocationShare.isEnabled(context)) return
        LocationDiagnostics.mark(context, LocationDiagnostics.WATCHDOG_AT)
        LocationShare.startService(context, fromBackground = true, tick = true)
        schedule(context)
    }

    companion object {
        private const val ACTION = "io.github.realfamousbae.staya.WATCHDOG"

        fun schedule(context: Context) {
            val alarms = context.getSystemService(AlarmManager::class.java)
            alarms.setAndAllowWhileIdle(
                AlarmManager.ELAPSED_REALTIME_WAKEUP,
                SystemClock.elapsedRealtime() + LocationPolicy.WATCHDOG_INTERVAL_MS,
                pending(context),
            )
        }

        fun cancel(context: Context) {
            context.getSystemService(AlarmManager::class.java).cancel(pending(context))
        }

        private fun pending(context: Context): PendingIntent = PendingIntent.getBroadcast(
            context,
            0,
            Intent(context, Watchdog::class.java).setAction(ACTION),
            PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
        )
    }
}
