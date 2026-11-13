package io.github.realfamousbae.staya.push

import android.app.Application
import android.content.Context
import androidx.work.*
import com.google.android.gms.common.GoogleApiAvailabilityLight
import com.google.android.gms.tasks.Tasks
import com.google.firebase.messaging.FirebaseMessaging
import io.github.realfamousbae.staya.AppCore
import io.github.realfamousbae.staya.BuildConfig
import io.github.realfamousbae.staya.net.Net
import org.json.JSONObject
import java.io.IOException
import java.util.concurrent.TimeUnit

/** Token acquisition is opt-in; preferences contain flags/times, never app keys. */
object FcmPush {
    private const val PREFS = "fcm"
    private const val WORK = "staya-fcm-mailbox"
    private const val REFRESH = "staya-fcm-refresh"
    fun isEnabled(context: Context) = context.getSharedPreferences(PREFS, 0).getBoolean("enabled", false)
    fun available(context: Context): Boolean = BuildConfig.FCM_CONFIGURED && runCatching {
        GoogleApiAvailabilityLight.getInstance().isGooglePlayServicesAvailable(context) == 0
    }.getOrDefault(false)
    private fun pendingCleanup(context: Context) = context.getSharedPreferences(PREFS, 0).getBoolean("cleanup", false)

    fun initialize(app: Application) {
        // Keep SDK auto-init disabled even after consent; token requests stay explicit.
        if (BuildConfig.FCM_CONFIGURED) runCatching { FirebaseMessaging.getInstance().isAutoInitEnabled = false }
        if (isEnabled(app)) ensurePeriodic(app) else runCatching { WorkManager.getInstance(app).cancelUniqueWork(REFRESH) }
        if (isEnabled(app) || pendingCleanup(app)) enqueue(app)
    }
    private fun ensurePeriodic(context: Context) {
        val request = PeriodicWorkRequest.Builder(PushSyncWorker::class.java, 12, TimeUnit.HOURS)
            .setConstraints(Constraints.Builder().setRequiredNetworkType(NetworkType.CONNECTED).build()).build()
        runCatching { WorkManager.getInstance(context).enqueueUniquePeriodicWork(REFRESH, ExistingPeriodicWorkPolicy.KEEP, request) }
    }
    fun enqueue(context: Context) {
        val request = OneTimeWorkRequest.Builder(PushSyncWorker::class.java)
            .setConstraints(Constraints.Builder().setRequiredNetworkType(NetworkType.CONNECTED).build())
            .setBackoffCriteria(BackoffPolicy.EXPONENTIAL, 30, TimeUnit.SECONDS).build()
        runCatching { WorkManager.getInstance(context).enqueueUniqueWork(WORK, ExistingWorkPolicy.KEEP, request) }
    }
    fun maybeRefresh(context: Context) {
        val prefs = context.getSharedPreferences(PREFS, 0)
        if (pendingCleanup(context) || (isEnabled(context) && System.currentTimeMillis() - prefs.getLong("registered", 0) > TimeUnit.DAYS.toMillis(1))) enqueue(context)
    }

    /** Call in Net.worker, including SDK operations, to serialize enable/disable. */
    fun enable(context: Context, core: uniffi.staya_core.StayaCore): Boolean {
        if (!available(context)) return false
        val (client, _) = Net.bind(core) ?: return false
        if (!JSONObject(client.get("/v1/capabilities")).optBoolean("fcm", false)) return false
        val messaging = FirebaseMessaging.getInstance()
        messaging.isAutoInitEnabled = false
        try {
            val token = Tasks.await(messaging.token, 15, TimeUnit.SECONDS)
            client.put("/v1/push", JSONObject().put("token", token).toString())
            if (!context.getSharedPreferences(PREFS, 0).edit().putBoolean("enabled", true).putBoolean("cleanup", false).putLong("registered", System.currentTimeMillis()).commit()) throw IOException("push preference unavailable")
            ensurePeriodic(context)
            enqueue(context)
            return true
        } catch (error: Exception) {
            messaging.isAutoInitEnabled = false
            runCatching { Tasks.await(messaging.deleteToken(), 15, TimeUnit.SECONDS) }
            runCatching { client.delete("/v1/push") }
            throw error
        }
    }
    fun disable(context: Context, core: uniffi.staya_core.StayaCore): Boolean {
        if (!context.getSharedPreferences(PREFS, 0).edit().putBoolean("enabled", false).putBoolean("cleanup", true).commit()) throw IOException("push preference unavailable")
        if (BuildConfig.FCM_CONFIGURED) runCatching { FirebaseMessaging.getInstance().isAutoInitEnabled = false }
        runCatching { WorkManager.getInstance(context).cancelUniqueWork(REFRESH) }
        val cleaned = cleanup(context, core)
        if (!cleaned) enqueue(context)
        return cleaned
    }
    private fun cleanup(context: Context, core: uniffi.staya_core.StayaCore): Boolean {
        val server = runCatching {
            val (client, _) = Net.bind(core) ?: throw IOException("no server")
            client.delete("/v1/push")
        }.isSuccess
        val provider = !BuildConfig.FCM_CONFIGURED || runCatching {
            FirebaseMessaging.getInstance().isAutoInitEnabled = false
            Tasks.await(FirebaseMessaging.getInstance().deleteToken(), 15, TimeUnit.SECONDS)
        }.isSuccess
        return server && provider && context.getSharedPreferences(PREFS, 0).edit().putBoolean("cleanup", false).commit()
    }

    /** Worker entry, invoked inside Net.worker. No credentials/tokens in work input. */
    fun synchronize(context: Context): Boolean {
        if (!isEnabled(context) && !pendingCleanup(context)) return true
        val core = AppCore.openBlocking(context) ?: return false
        if (!isEnabled(context)) return !pendingCleanup(context) || cleanup(context, core)
        if (!available(context)) return true // Existing location/mailbox paths remain operational.
        val (client, sync) = Net.bind(core) ?: return false
        if (!JSONObject(client.get("/v1/capabilities")).optBoolean("fcm", false)) return false
        FirebaseMessaging.getInstance().isAutoInitEnabled = false
        val token = Tasks.await(FirebaseMessaging.getInstance().token, 15, TimeUnit.SECONDS)
        client.put("/v1/push", JSONObject().put("token", token).toString())
        context.getSharedPreferences(PREFS, 0).edit().putLong("registered", System.currentTimeMillis()).apply()
        val events = sync.sync()
        if (events.any { it is uniffi.staya_core.CoreEvent.FriendAdded }) io.github.realfamousbae.staya.location.LocationShare.resend(core)
        return true
    }
}

class PushSyncWorker(context: Context, params: WorkerParameters) : Worker(context, params) {
    override fun doWork(): Result {
        (applicationContext as? Application)?.let {
            io.github.realfamousbae.staya.location.NativeSharingConsent.initialize(it)
            io.github.realfamousbae.staya.places.PlaceNotifications.initialize(it)
            io.github.realfamousbae.staya.location.RemoteLocationRequests.initialize(it)
        }
        val ok = runCatching { Net.worker.submit<Boolean> { FcmPush.synchronize(applicationContext) }.get(60, TimeUnit.SECONDS) }.getOrDefault(false)
        return if (ok) Result.success() else if (runAttemptCount < 3) Result.retry() else Result.failure()
    }
}
