package io.github.realfamousbae.staya.location

import android.annotation.SuppressLint
import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
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
import android.os.Handler
import android.os.IBinder
import android.os.Looper
import io.github.realfamousbae.staya.MainActivity
import io.github.realfamousbae.staya.R

/**
 * Сервис переднего плана типа `location` (задачи 1.3, 4.5). Стратегия A3
 * (`docs/measurements/stage1.md`): в покое — экономно раз в 5 минут; датчик значимого
 * движения включает точный режим (100 м), обратно — через 10 минут без движения.
 * Без Google Play Services: `LocationManager` (fused на API 31+, иначе сеть/GPS).
 *
 * Запуск никогда не роняет приложение: без разрешения или при запрете запуска из
 * фона сервис просто останавливается. `Location` никогда не печатать — в описании
 * координаты.
 */
class LocationService : Service() {
    private lateinit var lm: LocationManager
    private var sensors: SensorManager? = null
    private var listening = false
    private var highPower = false
    private var lastMovingAt = 0L

    private val listener = LocationListener { location -> onLocation(location) }

    // В точном режиме с шагом 100 м стоящий телефон не даёт замеров: без таймера
    // проверку «стоим» некому было бы вызвать, и точный режим не выключался бы.
    private val handler = Handler(Looper.getMainLooper())
    private val stillCheck = object : Runnable {
        override fun run() {
            checkStill()
            if (highPower) handler.postDelayed(this, STILL_CHECK_MS)
        }
    }

    private val motionTrigger = object : TriggerEventListener() {
        override fun onTrigger(event: TriggerEvent?) {
            // Датчик срабатывает один раз; взводим заново при возврате в экономный режим.
            if (!highPower) switchPower(high = true)
        }
    }

    override fun onCreate() {
        super.onCreate()
        lm = getSystemService(LocationManager::class.java)
        sensors = getSystemService(SensorManager::class.java)
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        if (!LocationShare.isEnabled(this) || !LocationShare.hasForegroundPermission(this) || !enterForeground()) {
            stopSelf()
            return START_NOT_STICKY
        }
        if (!listening) {
            stopUpdates()
            request(high = false)
            armMotion()
            listening = true
        }
        return START_STICKY
    }

    private fun enterForeground(): Boolean = try {
        ensureChannel()
        startForeground(NOTIFICATION_ID, notification(), ServiceInfo.FOREGROUND_SERVICE_TYPE_LOCATION)
        true
    } catch (_: SecurityException) {
        // Android 14+: из фона без «Разрешить всегда» тип location запрещён.
        false
    } catch (_: IllegalStateException) {
        // ForegroundServiceStartNotAllowedException (API 31+) — наследник IllegalStateException.
        false
    }

    private fun switchPower(high: Boolean) {
        if (high == highPower) return
        stopUpdates()
        request(high)
        listening = true
        handler.removeCallbacks(stillCheck)
        if (high) {
            lastMovingAt = System.currentTimeMillis()
            handler.postDelayed(stillCheck, STILL_CHECK_MS)
        } else {
            armMotion()
        }
    }

    @SuppressLint("MissingPermission") // Проверено в onStartCommand.
    private fun request(high: Boolean) {
        if (!LocationShare.hasForegroundPermission(this)) return
        highPower = high
        val provider = pickProvider(high)
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

    private fun pickProvider(high: Boolean): String {
        val has = { p: String -> runCatching { lm.allProviders.contains(p) }.getOrDefault(false) }
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S && lm.hasProvider(LocationManager.FUSED_PROVIDER)) {
            return LocationManager.FUSED_PROVIDER
        }
        return when {
            high && has(LocationManager.GPS_PROVIDER) -> LocationManager.GPS_PROVIDER
            has(LocationManager.NETWORK_PROVIDER) -> LocationManager.NETWORK_PROVIDER
            has(LocationManager.GPS_PROVIDER) -> LocationManager.GPS_PROVIDER
            else -> LocationManager.PASSIVE_PROVIDER
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
        if (location.hasSpeed() && location.speed >= 0.5f) lastMovingAt = System.currentTimeMillis()
        LocationShare.onFix(
            this,
            Fix(location.latitude, location.longitude, if (location.hasAccuracy()) location.accuracy else null, location.time),
        )
        checkStill()
    }

    /** Десять минут без движения — обратно в экономный режим и ждём датчик. */
    private fun checkStill() {
        if (highPower && System.currentTimeMillis() - lastMovingAt > STILL_TIMEOUT_MS) switchPower(high = false)
    }

    override fun onDestroy() {
        handler.removeCallbacks(stillCheck)
        stopUpdates()
        super.onDestroy()
    }

    override fun onBind(intent: Intent?): IBinder? = null

    private fun ensureChannel() {
        val nm = getSystemService(NotificationManager::class.java)
        if (nm.getNotificationChannel(CHANNEL) == null) {
            nm.createNotificationChannel(
                NotificationChannel(CHANNEL, getString(R.string.location_channel), NotificationManager.IMPORTANCE_LOW),
            )
        }
    }

    private fun notification(): Notification {
        val open = PendingIntent.getActivity(this, 0, Intent(this, MainActivity::class.java), PendingIntent.FLAG_IMMUTABLE)
        return Notification.Builder(this, CHANNEL)
            .setContentTitle(getString(R.string.location_notification_title))
            .setContentText(getString(R.string.location_notification_text))
            .setSmallIcon(R.drawable.ic_launcher_foreground)
            .setContentIntent(open)
            .setOngoing(true)
            .build()
    }

    private companion object {
        const val CHANNEL = "location"
        const val NOTIFICATION_ID = 1
        const val LOW_INTERVAL_MS = 5 * 60_000L
        const val HIGH_INTERVAL_MS = 10_000L
        const val HIGH_DISTANCE_M = 100f
        const val STILL_TIMEOUT_MS = 10 * 60_000L
        const val STILL_CHECK_MS = 60_000L
    }
}
