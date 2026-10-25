package io.github.realfamousbae.staya.location

import android.Manifest
import android.annotation.SuppressLint
import android.content.Context
import android.content.Intent
import android.os.Build
import android.os.PowerManager
import android.provider.Settings
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Switch
import androidx.compose.material3.SwitchDefaults
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.dp
import androidx.core.net.toUri
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.compose.LocalLifecycleOwner
import androidx.lifecycle.repeatOnLifecycle
import io.github.realfamousbae.staya.map.MapMath
import kotlinx.coroutines.delay
import io.github.realfamousbae.staya.ui.Ink
import io.github.realfamousbae.staya.ui.SectionTitle
import io.github.realfamousbae.staya.ui.SoftButton
import uniffi.staya_core.StayaCore

/**
 * «Делиться позицией» (задача 4.5). Разрешения запрашиваются только по нажатию:
 * сначала геолокация, затем «Разрешить всегда» (без неё Android 14+ не поднимет
 * сервис после перезагрузки), уведомление сервиса и исключение из оптимизации батареи.
 */
@Composable
fun SharingCard(core: StayaCore) {
    val context = LocalContext.current
    var enabled by remember { mutableStateOf(LocationShare.isEnabled(context)) }
    // Перечитать разрешения при возвращении из настроек.
    var resumes by remember { mutableIntStateOf(0) }
    val lifecycle = LocalLifecycleOwner.current.lifecycle
    LaunchedEffect(lifecycle) { lifecycle.repeatOnLifecycle(Lifecycle.State.RESUMED) { resumes++ } }

    val foreground = rememberLauncherForActivityResult(ActivityResultContracts.RequestMultiplePermissions()) { result ->
        resumes++
        if (result.values.any { it } && LocationShare.hasForegroundPermission(context)) {
            LocationShare.enable(context, core)
            enabled = true
        }
    }
    val single = rememberLauncherForActivityResult(ActivityResultContracts.RequestPermission()) { resumes++ }

    Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
        SectionTitle("передача")
        Row(verticalAlignment = Alignment.CenterVertically) {
            Column(modifier = Modifier.weight(1f)) {
                Text("Делиться позицией", style = MaterialTheme.typography.titleMedium)
                Text(
                    if (enabled) "Друзья видят, где ты" else "Выключено — друзья видят «скрыл(а) позицию»",
                    style = MaterialTheme.typography.bodySmall,
                    color = Ink.mist,
                )
            }
            Switch(colors = switchColors(), checked = enabled, onCheckedChange = { on ->
                if (!on) {
                    LocationShare.disable(context, core)
                    enabled = false
                } else if (LocationShare.hasForegroundPermission(context)) {
                    LocationShare.enable(context, core)
                    enabled = true
                } else {
                    foreground.launch(
                        arrayOf(Manifest.permission.ACCESS_FINE_LOCATION, Manifest.permission.ACCESS_COARSE_LOCATION),
                    )
                }
            })
        }
        if (!enabled) return@Column
        var frozen by remember { mutableStateOf(LocationShare.isFrozen(core)) }
        var freezeError by remember { mutableStateOf(false) }
        Row(verticalAlignment = Alignment.CenterVertically) {
            Column(modifier = Modifier.weight(1f)) {
                Text("Заморозить позицию здесь", style = MaterialTheme.typography.titleMedium)
                Text(
                    if (freezeError) {
                        "Пока нет ни одного замера — подожди, пока позиция определится."
                    } else if (frozen) {
                        "Друзья видят эту точку, куда бы ты ни пошёл(а)"
                    } else {
                        "Друзья увидят последнюю точку, пока не снимешь"
                    },
                    style = MaterialTheme.typography.bodySmall,
                    color = Ink.mist,
                )
            }
            Switch(colors = switchColors(), checked = frozen, onCheckedChange = { on ->
                LocationShare.setFrozen(core, on) { ok ->
                    freezeError = on && !ok
                    if (ok) frozen = on
                }
            })
        }
        Diagnostics(resumes)
        // Пересчитываются при каждом возвращении на экран (например, из настроек).
        val background = remember(resumes) { LocationShare.hasBackgroundPermission(context) }
        val notifications = remember(resumes) {
            Build.VERSION.SDK_INT < Build.VERSION_CODES.TIRAMISU ||
                LocationShare.granted(context, Manifest.permission.POST_NOTIFICATIONS)
        }
        val exempt = remember(resumes) {
            context.getSystemService(PowerManager::class.java).isIgnoringBatteryOptimizations(context.packageName)
        }
        if (!background) {
            Text(
                "Чтобы позиция уходила, когда приложение закрыто, выбери «Разрешить всегда».",
                style = MaterialTheme.typography.bodySmall,
                    color = Ink.mist,
            )
            // Android 11+: система открывает настройки разрешения.
            SoftButton(onClick = { single.launch(Manifest.permission.ACCESS_BACKGROUND_LOCATION) }) {
                Text("Геолокация «Разрешить всегда»")
            }
        }
        if (!notifications && Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
            SoftButton(onClick = { single.launch(Manifest.permission.POST_NOTIFICATIONS) }) {
                Text("Разрешить уведомление")
            }
        }
        if (!exempt) {
            Text(
                "Без исключения из оптимизации батареи система может останавливать передачу.",
                style = MaterialTheme.typography.bodySmall,
                    color = Ink.mist,
            )
            SoftButton(onClick = { requestBatteryExemption(context) }) { Text("Отключить оптимизацию батареи") }
            Text(
                "Xiaomi/Poco: «Автозапуск» — вкл, «Контроль активности» — «Нет ограничений». " +
                    "Samsung: «Батарея» — «Неограниченно», добавить в «Приложения, которые никогда не спят».",
                style = MaterialTheme.typography.bodySmall,
                    color = Ink.mist,
            )
            SoftButton(onClick = {
                context.startActivity(
                    Intent(Settings.ACTION_APPLICATION_DETAILS_SETTINGS, "package:${context.packageName}".toUri()),
                )
            }) { Text("Настройки приложения") }
        }
    }
}

/**
 * Что происходит с передачей (5.1): жив ли сервис, когда был замер и отправка, что
 * не удалось. Времена без координат — по ним видно, «лежал телефон» или «сервис умер».
 */
@Composable
private fun Diagnostics(resumes: Int) {
    val context = LocalContext.current
    // Сервис стартует и шлёт асинхронно: перечитываем раз в несколько секунд, пока карточка видна.
    var tick by remember { mutableIntStateOf(0) }
    LaunchedEffect(Unit) {
        while (true) {
            delay(5_000)
            tick++
        }
    }
    val d = remember(resumes, tick) { LocationDiagnostics.snapshot(context) }
    val running = remember(resumes, tick) { LocationService.running }
    val now = System.currentTimeMillis() / 1000
    fun ago(ms: Long?) = ms?.let { MapMath.ageLabel(now, it / 1000) } ?: "ещё не было"
    val lines = buildList {
        add(
            if (running) {
                "Передача работает"
            } else if (d.serviceFailed != null) {
                "Передача не запущена: ${d.serviceFailed} (${ago(d.serviceFailedAt)})"
            } else {
                "Передача не запущена — открой приложение заново или проверь настройки ниже"
            },
        )
        add("Замер: ${ago(d.fixAt)} · отправка: ${ago(d.sentAt)}")
        if (d.sendError != null) add("Не удалось отправить: ${d.sendError} (${ago(d.sendErrorAt)}) — уйдёт при следующей попытке")
        d.watchdogAt?.let { add("Сторож проверял: ${ago(it)}") }
        if (d.drop != null) add("Пакет от друга отброшен: ${d.drop} (${ago(d.dropAt)})")
    }
    Column(verticalArrangement = Arrangement.spacedBy(2.dp)) {
        lines.forEachIndexed { i, line -> Text(line, style = MaterialTheme.typography.bodySmall, color = if (i == 0 && !running) Ink.danger else Ink.mist) }
    }
}

/**
 * Прямой запрос исключения из оптимизации батареи. Lint (BatteryLife) против — это
 * правило Google Play; Staya ставится APK, а без исключения производители телефонов
 * останавливают сервис, и друзья перестают видеть позицию.
 */
@SuppressLint("BatteryLife")
private fun requestBatteryExemption(context: Context) {
    context.startActivity(
        Intent(Settings.ACTION_REQUEST_IGNORE_BATTERY_OPTIMIZATIONS, "package:${context.packageName}".toUri()),
    )
}

@Composable
private fun switchColors() = SwitchDefaults.colors(
    checkedThumbColor = Ink.ink,
    checkedTrackColor = Ink.mint,
    uncheckedThumbColor = Ink.mist,
    uncheckedTrackColor = Ink.ink2,
    uncheckedBorderColor = Ink.ink3,
)
