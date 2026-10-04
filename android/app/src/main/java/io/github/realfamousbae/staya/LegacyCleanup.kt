package io.github.realfamousbae.staya

import android.content.Context
import io.github.realfamousbae.staya.secure.SecretStore
import java.io.File

/**
 * Остатки прототипа замеров этапа 1 (удалён в 4.5): токен сборщика метрик, очередь
 * и настройки. Обновление приложения их не стирает; удаляем при каждом запуске —
 * это дёшево, а файлов уже нет.
 */
object LegacyCleanup {
    fun run(context: Context) {
        runCatching { SecretStore(context, "probe-token").delete() }
        runCatching { File(context.noBackupFilesDir, "probe-queue.txt").delete() }
        runCatching { context.deleteSharedPreferences("probe") }
    }
}
