package io.github.realfamousbae.staya.probe

import android.Manifest
import android.annotation.SuppressLint
import android.app.AlarmManager
import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.Context
import android.content.Intent
import android.content.pm.ServiceInfo
import android.hardware.Sensor
import android.hardware.SensorManager
import android.hardware.TriggerEvent
import android.hardware.TriggerEventListener
import android.location.Location
import android.location.LocationListener
import android.location.LocationManager
import android.location.LocationRequest
import android.os.Build
import android.os.IBinder
import android.os.Looper
import android.os.SystemClock
import io.github.realfamousbae.staya.MainActivity
import io.github.realfamousbae.staya.R

/**
 * Сервис переднего плана типа `location` (задача 1.3).
 *
 * Запуск никогда не роняет приложение: без разрешения на геолокацию или при
 * запрете запуска из фона сервис записывает ошибку и останавливается — падение у
 * друга стоило бы дня замеров, о котором мы бы даже не узнали.
 */
class LocationProbeService : Service() {
    private lateinit var lm: LocationManager
    private var sensors: SensorManager? = null
    private var listening = false
    private var highPower = false
    private var expectStartFix = false
    private var lastMovingAt = 0L
    private var activeProvider: ProbeRecord.Provider? = null

    private val listener = LocationListener { location -> onLocation(location) }

    private val motionTrigger = object : TriggerEventListener() {
        override fun onTrigger(event: TriggerEvent?) {
            // Датчик срабатывает один раз — после переключения взводим заново при остановке.
            if (Probe.strategy == Probe.Strategy.A3 && !highPower) {
                Probe.record(ProbeRecord.Trigger.MOTION, provider = activeProvider)
                switchPower(high = true, record = true)
            }
        }
    }

    override fun onCreate() {
        super.onCreate()
        Probe.init(this)
        lm = getSystemService(LocationManager::class.java)
        sensors = getSystemService(SensorManager::class.java)
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        if (!Probe.running || !hasLocationPermission()) {
            if (!hasLocationPermission()) Probe.lastError = "нет разрешения на геолокацию"
            stopSelf()
            return START_NOT_STICKY
        }
        if (!enterForeground()) {
            stopSelf()
            return START_NOT_STICKY
        }
        when (intent?.action) {
            ACTION_HEARTBEAT -> onHeartbeat()
            else -> {
                // Явный запуск с экрана или после загрузки/перезапуска системой (intent == null).
                expectStartFix = intent?.getBooleanExtra(EXTRA_FROM_UI, false) == true
                if (!listening) applyStrategy()
            }
        }
        scheduleHeartbeat(this)
        return START_STICKY
    }

    private fun enterForeground(): Boolean = try {
        ensureChannel()
        startForeground(NOTIFICATION_ID, notification(), ServiceInfo.FOREGROUND_SERVICE_TYPE_LOCATION)
        Probe.lastError = null
        true
    } catch (e: SecurityException) {
        // Android 14+: из фона без «Разрешить всегда» тип location запрещён.
        Probe.lastError = "запуск запрещён: нужна геолокация «Разрешить всегда»"
        false
    } catch (e: IllegalStateException) {
        // ForegroundServiceStartNotAllowedException (API 31+) — наследник IllegalStateException.
        Probe.lastError = "запуск из фона запрещён системой"
        false
    }

    // --- Стратегии ---------------------------------------------------------

    private fun applyStrategy() {
        stopUpdates()
        when (Probe.strategy) {
            Probe.Strategy.A1 -> request(high = false)
            Probe.Strategy.A2 -> request(high = true)
            Probe.Strategy.A3 -> {
                request(high = false)
                armMotion()
            }
        }
        listening = true
    }

    private fun switchPower(high: Boolean, record: Boolean) {
        if (high == highPower) return
        stopUpdates()
        request(high)
        listening = true
        if (record) {
            Probe.record(
                if (high) ProbeRecord.Trigger.CONTINUOUS_START else ProbeRecord.Trigger.CONTINUOUS_STOP,
                provider = activeProvider,
                force = true,
            )
        }
        if (high) lastMovingAt = System.currentTimeMillis() else armMotion()
    }

    @SuppressLint("MissingPermission") // Проверено в onStartCommand через hasLocationPermission().
    private fun request(high: Boolean) {
        if (!hasLocationPermission()) return
        val (provider, wire) = pickProvider(high)
        activeProvider = wire
        highPower = high
        Probe.continuousOn = high
        val interval = if (high) HIGH_INTERVAL_MS else LOW_INTERVAL_MS
        val distance = if (high) HIGH_DISTANCE_M else 0f
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
            val quality = if (high) LocationRequest.QUALITY_HIGH_ACCURACY else LocationRequest.QUALITY_LOW_POWER
            val req = LocationRequest.Builder(interval).setMinUpdateDistanceMeters(distance).setQuality(quality).build()
            lm.requestLocationUpdates(provider, req, mainExecutor, listener)
        } else {
            lm.requestLocationUpdates(provider, interval, distance, listener, Looper.getMainLooper())
        }
    }

    /** Без Google Play Services: fused (API 31+, если есть), иначе GPS/сеть. */
    private fun pickProvider(high: Boolean): Pair<String, ProbeRecord.Provider> {
        val has = { p: String -> runCatching { lm.allProviders.contains(p) }.getOrDefault(false) }
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S && lm.hasProvider(LocationManager.FUSED_PROVIDER)) {
            return LocationManager.FUSED_PROVIDER to ProbeRecord.Provider.FUSED
        }
        return when {
            high && has(LocationManager.GPS_PROVIDER) -> LocationManager.GPS_PROVIDER to ProbeRecord.Provider.GPS
            has(LocationManager.NETWORK_PROVIDER) -> LocationManager.NETWORK_PROVIDER to ProbeRecord.Provider.NETWORK
            has(LocationManager.GPS_PROVIDER) -> LocationManager.GPS_PROVIDER to ProbeRecord.Provider.GPS
            else -> LocationManager.PASSIVE_PROVIDER to ProbeRecord.Provider.PASSIVE
        }
    }

    private fun armMotion() {
        val sm = sensors ?: return
        val sensor = sm.getDefaultSensor(Sensor.TYPE_SIGNIFICANT_MOTION) ?: return
        sm.requestTriggerSensor(motionTrigger, sensor)
    }

    private fun stopUpdates() {
        lm.removeUpdates(listener)
        sensors?.getDefaultSensor(Sensor.TYPE_SIGNIFICANT_MOTION)?.let { sensors?.cancelTriggerSensor(motionTrigger, it) }
        listening = false
    }

    private fun onLocation(location: Location) {
        val trigger = if (expectStartFix) {
            expectStartFix = false
            ProbeRecord.Trigger.FOREGROUND
        } else {
            ProbeRecord.Trigger.CONTINUOUS
        }
        if (location.hasSpeed() && location.speed >= 0.5f) lastMovingAt = System.currentTimeMillis()
        Probe.record(trigger, location, activeProvider)
        checkStill()
    }

    private fun onHeartbeat() {
        Probe.record(ProbeRecord.Trigger.TIMER, provider = activeProvider)
        if (!listening) applyStrategy()
        checkStill()
    }

    /** A3: десять минут без движения — обратно в экономный режим и ждём датчик. */
    private fun checkStill() {
        if (Probe.strategy == Probe.Strategy.A3 && highPower &&
            System.currentTimeMillis() - lastMovingAt > STILL_TIMEOUT_MS
        ) {
            switchPower(high = false, record = true)
        }
    }

    override fun onDestroy() {
        stopUpdates()
        Probe.continuousOn = false
        if (!Probe.running) cancelHeartbeat(this)
        super.onDestroy()
    }

    override fun onBind(intent: Intent?): IBinder? = null

    private fun hasLocationPermission() =
        Probe.granted(this, Manifest.permission.ACCESS_FINE_LOCATION) ||
            Probe.granted(this, Manifest.permission.ACCESS_COARSE_LOCATION)

    // --- Уведомление -------------------------------------------------------

    private fun ensureChannel() {
        val nm = getSystemService(NotificationManager::class.java)
        if (nm.getNotificationChannel(CHANNEL) == null) {
            nm.createNotificationChannel(
                NotificationChannel(CHANNEL, getString(R.string.probe_channel), NotificationManager.IMPORTANCE_LOW),
            )
        }
    }

    private fun notification(): Notification {
        val open = PendingIntent.getActivity(
            this, 0, Intent(this, MainActivity::class.java), PendingIntent.FLAG_IMMUTABLE,
        )
        return Notification.Builder(this, CHANNEL)
            .setContentTitle(getString(R.string.probe_notification_title))
            .setContentText(getString(R.string.probe_notification_text, Probe.strategy.title))
            .setSmallIcon(R.drawable.ic_launcher_foreground)
            .setContentIntent(open)
            .setOngoing(true)
            .build()
    }

    companion object {
        const val ACTION_START = "io.github.realfamousbae.staya.probe.START"
        const val ACTION_HEARTBEAT = "io.github.realfamousbae.staya.probe.HEARTBEAT"
        const val EXTRA_FROM_UI = "from_ui"
        private const val CHANNEL = "probe"
        private const val NOTIFICATION_ID = 1
        private const val LOW_INTERVAL_MS = 5 * 60_000L
        private const val HIGH_INTERVAL_MS = 10_000L
        private const val HIGH_DISTANCE_M = 100f
        private const val STILL_TIMEOUT_MS = 10 * 60_000L
        private const val HEARTBEAT_MS = 15 * 60_000L

        /**
         * Пульс раз в ~15 минут: неточный будильник, работает и в Doze, не требует
         * разрешения на точные будильники. По пропускам пульса видно, когда процесс убили.
         */
        fun scheduleHeartbeat(context: Context) {
            val am = context.getSystemService(AlarmManager::class.java)
            am.setAndAllowWhileIdle(
                AlarmManager.ELAPSED_REALTIME_WAKEUP,
                SystemClock.elapsedRealtime() + HEARTBEAT_MS,
                heartbeatIntent(context),
            )
        }

        fun cancelHeartbeat(context: Context) {
            context.getSystemService(AlarmManager::class.java).cancel(heartbeatIntent(context))
        }

        private fun heartbeatIntent(context: Context): PendingIntent = PendingIntent.getBroadcast(
            context, 0, Intent(context, HeartbeatReceiver::class.java), PendingIntent.FLAG_IMMUTABLE,
        )
    }
}
