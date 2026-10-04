package io.github.realfamousbae.staya.probe

import android.Manifest
import android.annotation.SuppressLint
import android.app.ActivityManager
import android.app.usage.UsageStatsManager
import android.content.Context
import android.content.SharedPreferences
import android.content.pm.PackageManager
import android.hardware.Sensor
import android.hardware.SensorManager
import android.location.Location
import android.os.BatteryManager
import android.os.PowerManager
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.core.content.edit
import io.github.realfamousbae.staya.secure.SecretRead
import io.github.realfamousbae.staya.secure.SecretStore
import java.io.File
import java.util.UUID
import java.util.concurrent.Executors

/**
 * Прототип фоновой геолокации этапа 1 для Android (docs/PLAN.md, задача 1.3).
 *
 * Общий для сервиса, приёмников и экрана регистратор метрик. Координаты не
 * сохраняются и не отправляются — только `ProbeRecord`. Никогда не логировать
 * `Location`: в описании есть координаты.
 *
 * На Android сервис переднего плана держит процесс живым, поэтому главный
 * вопрос — убивает ли его производитель телефона. Для этого пульс (`timer`)
 * раз в ~15 минут и метка `relaunched` на первой записи нового процесса.
 *
 * StaticFieldLeak подавлен осознанно: в поле хранится только `applicationContext`,
 * он живёт столько же, сколько процесс, — утечки активности нет.
 */
@SuppressLint("StaticFieldLeak")
object Probe {
    enum class Strategy(val id: String, val title: String) {
        /** Экономно: сеть/низкое потребление, раз в 5 минут. */
        A1("a1", "A1 · экономно (5 мин)"),
        /** Непрерывно: высокая точность, каждые 100 м. */
        A2("a2", "A2 · непрерывно (100 м)"),
        /** Адаптивно: экономно в покое, непрерывно после датчика значимого движения. */
        A3("a3", "A3 · адаптивно"),
    }

    private lateinit var app: Context
    private lateinit var prefs: SharedPreferences
    private val executor = Executors.newSingleThreadExecutor()

    /** Трогать только на [executor]. */
    private lateinit var queue: ProbeQueue
    private lateinit var tokenStore: SecretStore

    /** Экран приложения сейчас на виду. */
    @Volatile var uiVisible = false

    /** Первая запись этого процесса ещё не сделана. */
    @Volatile private var freshProcess = true
    private var eventsSinceLast = 0L
    private var lastSendAt = 0L
    private var prevSendMs: Long? = null
    private var prevSendFailures = 0L
    private var sending = false

    /** Непрерывные обновления шлём не чаще, чтобы радио не перекрыло расход геолокации. */
    private const val MIN_SEND_INTERVAL_MS = 45_000L

    // Состояние для экрана (снимки Compose можно писать с любого потока).
    var strategy by mutableStateOf(Strategy.A1)
        private set
    var running by mutableStateOf(false)
        private set
    var continuousOn by mutableStateOf(false)
    var eventsTotal by mutableIntStateOf(0)
        private set
    var sentTotal by mutableIntStateOf(0)
        private set
    var failedTotal by mutableIntStateOf(0)
        private set
    var queued by mutableIntStateOf(0)
        private set
    var lastStatus by mutableStateOf("—")
        private set
    var lastEventAt by mutableStateOf<Long?>(null)
        private set
    var lastError by mutableStateOf<String?>(null)

    @Synchronized
    fun init(context: Context) {
        if (::app.isInitialized) return
        app = context.applicationContext
        prefs = app.getSharedPreferences("probe", Context.MODE_PRIVATE)
        strategy = Strategy.entries.firstOrNull { it.id == prefs.getString(K_STRATEGY, null) } ?: Strategy.A1
        running = prefs.getBoolean(K_RUNNING, false)
        eventsTotal = prefs.getInt(K_EVENTS, 0)
        sentTotal = prefs.getInt(K_SENT, 0)
        failedTotal = prefs.getInt(K_FAILED, 0)
        tokenStore = SecretStore(app, "probe-token")
        executor.execute {
            queue = ProbeQueue(File(app.noBackupFilesDir, "probe-queue.txt"))
            queued = queue.size
        }
    }

    val deviceId: String
        get() = prefs.getString(K_DEVICE, null) ?: ("and-" + UUID.randomUUID().toString().take(8)).also {
            prefs.edit { putString(K_DEVICE, it) }
        }

    fun select(s: Strategy) {
        strategy = s
        prefs.edit { putString(K_STRATEGY, s.id) }
    }

    fun changeRunning(on: Boolean) {
        running = on
        prefs.edit { putBoolean(K_RUNNING, on) }
    }

    fun saveToken(token: String): Boolean =
        runCatching { tokenStore.replace(token.trim().toByteArray(Charsets.UTF_8)); true }.getOrDefault(false)

    private fun loadToken(): String? =
        (tokenStore.read() as? SecretRead.Found)?.value?.toString(Charsets.UTF_8)

    val hasToken: Boolean get() = !loadToken().isNullOrEmpty()

    fun resetCounters() {
        eventsTotal = 0; sentTotal = 0; failedTotal = 0
        prefs.edit { remove(K_EVENTS).remove(K_SENT).remove(K_FAILED) }
    }

    /**
     * Записывает событие и отправляет очередь. `fromUi` — действие с экрана, а не
     * пробуждение системой.
     */
    fun record(
        trigger: ProbeRecord.Trigger,
        location: Location? = null,
        provider: ProbeRecord.Provider? = null,
        fromUi: Boolean = false,
        force: Boolean = false,
    ) {
        val appState = when {
            uiVisible -> ProbeRecord.AppState.FOREGROUND
            freshProcess && !fromUi -> ProbeRecord.AppState.RELAUNCHED
            else -> ProbeRecord.AppState.BACKGROUND
        }
        freshProcess = false
        val now = System.currentTimeMillis()
        eventsTotal += 1
        prefs.edit { putInt(K_EVENTS, eventsTotal) }
        lastEventAt = now
        val env = environment()
        executor.execute {
            eventsSinceLast += 1
            if (trigger == ProbeRecord.Trigger.CONTINUOUS && !force && now - lastSendAt < MIN_SEND_INTERVAL_MS) {
                return@execute
            }
            val r = ProbeRecord(
                device = deviceId,
                strategy = strategy.id,
                eventTs = now / 1000,
                trigger = trigger,
                appState = appState,
                accuracyM = location?.takeIf { it.hasAccuracy() }?.let { ProbeRecord.accuracy(it.accuracy) },
                speed = location?.takeIf { it.hasSpeed() }?.let { ProbeRecord.Speed.of(it.speed) }
                    ?: ProbeRecord.Speed.UNKNOWN,
                batteryPct = env.battery,
                charging = env.charging,
                lowPower = env.lowPower,
                prevSendMs = prevSendMs,
                prevSendFailures = prevSendFailures,
                auth = env.auth,
                precise = env.precise,
                eventsSinceLast = eventsSinceLast,
                batteryOptExempt = env.exempt,
                bgRestricted = env.bgRestricted,
                standbyBucket = env.bucket,
                doze = env.doze,
                provider = provider,
                sigMotion = env.sigMotion,
            )
            eventsSinceLast = 0
            queue.append(r.toJson())
            queued = queue.size
            flush()
        }
    }

    /** На [executor]. Отправленное удаляется по id — добавленное во время отправки не теряется. */
    private fun flush() {
        if (sending) return
        val token = loadToken()
        if (token.isNullOrEmpty()) {
            lastStatus = "нет токена"
            return
        }
        sending = true
        lastSendAt = System.currentTimeMillis()
        try {
            var budget = 8
            while (budget-- > 0) {
                val item = queue.peek(1).firstOrNull() ?: break
                val result = ProbeClient.send(item.json, token)
                lastStatus = result.status?.let { "HTTP $it" } ?: "нет ответа"
                if (result.ok) {
                    prevSendMs = result.millis
                    prevSendFailures = 0
                    queue.remove(item.id)
                    sentTotal += 1
                    prefs.edit { putInt(K_SENT, sentTotal) }
                } else {
                    prevSendMs = null
                    prevSendFailures += 1
                    failedTotal += 1
                    prefs.edit { putInt(K_FAILED, failedTotal) }
                    // 4xx (кроме 401) — запись не примут никогда; не держим её в начале очереди.
                    val s = result.status
                    if (s != null && s in 400..499 && s != 401) {
                        queue.remove(item.id)
                        continue
                    }
                    break
                }
            }
        } finally {
            sending = false
            queued = queue.size
        }
    }

    // --- Окружение ---------------------------------------------------------

    private class Env(
        val battery: Int, val charging: Boolean, val lowPower: Boolean,
        val auth: ProbeRecord.Auth, val precise: Boolean,
        val exempt: Boolean?, val bgRestricted: Boolean?, val bucket: ProbeRecord.StandbyBucket?,
        val doze: Boolean?, val sigMotion: Boolean?,
    )

    private fun environment(): Env {
        val bm = app.getSystemService(BatteryManager::class.java)
        val pm = app.getSystemService(PowerManager::class.java)
        val am = app.getSystemService(ActivityManager::class.java)
        val usm = app.getSystemService(UsageStatsManager::class.java)
        val sm = app.getSystemService(SensorManager::class.java)
        return Env(
            battery = bm?.getIntProperty(BatteryManager.BATTERY_PROPERTY_CAPACITY)?.coerceIn(0, 100) ?: 0,
            charging = bm?.isCharging ?: false,
            lowPower = pm?.isPowerSaveMode ?: false,
            auth = auth(app),
            precise = granted(app, Manifest.permission.ACCESS_FINE_LOCATION),
            exempt = pm?.isIgnoringBatteryOptimizations(app.packageName),
            bgRestricted = am?.isBackgroundRestricted,
            bucket = usm?.appStandbyBucket?.let(::bucket),
            doze = pm?.isDeviceIdleMode,
            sigMotion = sm?.getDefaultSensor(Sensor.TYPE_SIGNIFICANT_MOTION) != null,
        )
    }

    fun auth(context: Context): ProbeRecord.Auth = when {
        granted(context, Manifest.permission.ACCESS_BACKGROUND_LOCATION) -> ProbeRecord.Auth.ALWAYS
        granted(context, Manifest.permission.ACCESS_FINE_LOCATION) ||
            granted(context, Manifest.permission.ACCESS_COARSE_LOCATION) -> ProbeRecord.Auth.WHEN_IN_USE
        else -> ProbeRecord.Auth.DENIED
    }

    fun granted(context: Context, permission: String): Boolean =
        context.checkSelfPermission(permission) == PackageManager.PERMISSION_GRANTED

    private fun bucket(value: Int): ProbeRecord.StandbyBucket = when (value) {
        5 -> ProbeRecord.StandbyBucket.EXEMPTED // STANDBY_BUCKET_EXEMPTED (скрытая константа)
        UsageStatsManager.STANDBY_BUCKET_ACTIVE -> ProbeRecord.StandbyBucket.ACTIVE
        UsageStatsManager.STANDBY_BUCKET_WORKING_SET -> ProbeRecord.StandbyBucket.WORKING_SET
        UsageStatsManager.STANDBY_BUCKET_FREQUENT -> ProbeRecord.StandbyBucket.FREQUENT
        UsageStatsManager.STANDBY_BUCKET_RARE -> ProbeRecord.StandbyBucket.RARE
        UsageStatsManager.STANDBY_BUCKET_RESTRICTED -> ProbeRecord.StandbyBucket.RESTRICTED
        else -> ProbeRecord.StandbyBucket.UNKNOWN
    }

    private const val K_STRATEGY = "strategy"
    private const val K_RUNNING = "running"
    private const val K_DEVICE = "device"
    private const val K_EVENTS = "events"
    private const val K_SENT = "sent"
    private const val K_FAILED = "failed"
}
