package io.github.realfamousbae.staya.secure

import android.app.Activity
import android.app.KeyguardManager
import android.hardware.biometrics.BiometricPrompt
import android.os.CancellationSignal
import android.view.WindowManager
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue

/** UI lock only: background location must retain access to its existing Keystore key. */
object AppLock {
    var enabled by mutableStateOf(false)
        private set
    var locked by mutableStateOf(false)
        private set
    var busy by mutableStateOf(false)
        private set
    var message by mutableStateOf<String?>(null)
        private set
    private var initialized = false
    private val attempts = UnlockAttempt()
    private var cancellation: CancellationSignal? = null

    fun initialize(activity: Activity) {
        if (!initialized) {
            enabled = activity.getSharedPreferences("app-lock", 0).getBoolean("enabled", false)
            locked = enabled
            initialized = true
        }
        protectWindow(activity)
    }

    fun background() {
        attempts.invalidate()
        cancellation?.cancel()
        cancellation = null
        busy = false
        locked = enabled
    }

    fun unlock(activity: Activity) = authenticate(activity, null)
    fun change(activity: Activity) = authenticate(activity, !enabled)

    @Suppress("DEPRECATION") // API 29 is supported; the platform dialog includes device credential fallback.
    private fun authenticate(activity: Activity, newEnabled: Boolean?) {
        if (busy) return
        if (!activity.getSystemService(KeyguardManager::class.java).isDeviceSecure) {
            message = "Сначала настрой блокировку экрана в настройках телефона."
            return
        }
        val attempt = attempts.begin()
        val signal = CancellationSignal()
        cancellation = signal
        busy = true
        message = null
        val prompt = BiometricPrompt.Builder(activity)
            .setTitle("Открыть Staya")
            .setDescription("Подтверди вход биометрией или кодом телефона")
            .setDeviceCredentialAllowed(true)
            .build()
        prompt.authenticate(signal, activity.mainExecutor, object : BiometricPrompt.AuthenticationCallback() {
            override fun onAuthenticationSucceeded(result: BiometricPrompt.AuthenticationResult) {
                if (!attempts.finish(attempt)) return
                busy = false
                cancellation = null
                if (newEnabled != null) {
                    // A setting flag is not secret; database keys never enter preferences.
                    val saved = activity.getSharedPreferences("app-lock", 0).edit().putBoolean("enabled", newEnabled).commit()
                    if (!saved) {
                        message = "Не удалось сохранить настройку. Попробуй ещё раз."
                        return
                    }
                    enabled = newEnabled
                }
                locked = false
                protectWindow(activity)
            }
            override fun onAuthenticationError(errorCode: Int, errString: CharSequence) {
                if (!attempts.finish(attempt)) return
                busy = false
                cancellation = null
                message = "Вход не подтверждён. Можно попробовать ещё раз."
            }
        })
    }

    private fun protectWindow(activity: Activity) {
        if (enabled) activity.window.addFlags(WindowManager.LayoutParams.FLAG_SECURE)
        else activity.window.clearFlags(WindowManager.LayoutParams.FLAG_SECURE)
    }
}
