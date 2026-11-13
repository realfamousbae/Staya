package io.github.realfamousbae.staya.push

import android.os.Handler
import android.os.Looper
import androidx.compose.material3.Text
import androidx.compose.runtime.*
import androidx.compose.ui.platform.LocalContext
import io.github.realfamousbae.staya.BuildConfig
import io.github.realfamousbae.staya.net.Net
import io.github.realfamousbae.staya.ui.Ink
import io.github.realfamousbae.staya.ui.ListRow
import uniffi.staya_core.StayaCore

@Composable
fun FcmSetting(core: StayaCore) {
    val app = LocalContext.current.applicationContext
    var enabled by remember { mutableStateOf(FcmPush.isEnabled(app)) }
    var busy by remember { mutableStateOf(false) }
    var message by remember { mutableStateOf<String?>(null) }
    val main = remember { Handler(Looper.getMainLooper()) }
    val available = remember { FcmPush.available(app) }
    val subtitle = when {
        enabled -> "Включены"
        !BuildConfig.FCM_CONFIGURED -> "Не настроены для этой сборки"
        !available -> "Недоступны без Google Play"
        else -> "Выключены"
    }
    ListRow("Пуш-обновления через Google", subtitle, enabled = !busy && (enabled || available)) {
        busy = true
        val desired = !enabled
        Net.worker.execute {
            val result = runCatching { if (desired) FcmPush.enable(app, core) else FcmPush.disable(app, core) }
            main.post {
                enabled = FcmPush.isEnabled(app)
                message = when {
                    result.isFailure -> "Не удалось завершить настройку. Проверь связь и попробуй ещё раз."
                    desired && result.getOrDefault(false) -> "Пуш-обновления включены. Доставка может задерживаться в режиме сна."
                    desired -> "На сервере пока не настроены пуш-обновления."
                    !result.getOrDefault(false) -> "Выключены на телефоне. Удаление адреса доставки повторится при связи."
                    else -> "Пуш-обновления выключены."
                }
                busy = false
            }
        }
    }
    Text("Google видит идентификатор доставки и время. Координаты и имена в push не передаются.", color = Ink.mist)
    message?.let { Text(it, color = Ink.mist) }
}
