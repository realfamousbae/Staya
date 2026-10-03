package io.github.realfamousbae.staya.probe

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent

/**
 * Пульс от будильника. Если сервис жив, событие записывает он; если процесс был
 * убит и поднят будильником, сервис из фона не запустить — пишем пульс сами
 * (с меткой relaunched) и ошибку, это и есть данные о «убийствах».
 */
class HeartbeatReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        Probe.init(context)
        if (!Probe.running) return
        val started = runCatching {
            context.startService(
                Intent(context, LocationProbeService::class.java).setAction(LocationProbeService.ACTION_HEARTBEAT),
            )
        }.isSuccess
        if (!started) {
            Probe.lastError = "сервис был остановлен системой"
            Probe.record(ProbeRecord.Trigger.TIMER)
        }
        LocationProbeService.scheduleHeartbeat(context)
    }
}

/**
 * Загрузка телефона и обновление приложения. Android 14+ разрешает отсюда запуск
 * сервиса типа location (Android 15 ограничил другие типы), но только с
 * геолокацией «Разрешить всегда» — иначе SecurityException, который ловим.
 */
class BootReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        if (intent.action != Intent.ACTION_BOOT_COMPLETED && intent.action != Intent.ACTION_MY_PACKAGE_REPLACED) return
        Probe.init(context)
        if (!Probe.running) return
        Probe.record(ProbeRecord.Trigger.BOOT)
        runCatching {
            context.startForegroundService(
                Intent(context, LocationProbeService::class.java).setAction(LocationProbeService.ACTION_START),
            )
        }.onFailure { Probe.lastError = "после загрузки сервис не запустился" }
        LocationProbeService.scheduleHeartbeat(context)
    }
}
