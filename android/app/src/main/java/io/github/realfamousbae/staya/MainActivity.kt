package io.github.realfamousbae.staya

import android.Manifest
import android.annotation.SuppressLint
import android.content.Intent
import android.os.Build
import android.os.Bundle
import android.os.PowerManager
import android.provider.Settings
import androidx.activity.ComponentActivity
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.safeDrawingPadding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Button
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.RadioButton
import androidx.compose.material3.Surface
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.runtime.LaunchedEffect
import io.github.realfamousbae.staya.ui.AppModel
import io.github.realfamousbae.staya.ui.HomeScreen
import io.github.realfamousbae.staya.ui.OnboardingScreen
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.unit.dp
import androidx.core.net.toUri
import io.github.realfamousbae.staya.probe.LocationProbeService
import io.github.realfamousbae.staya.probe.Probe
import io.github.realfamousbae.staya.probe.ProbeRecord
import java.text.DateFormat
import java.util.Date
import uniffi.staya_core.coreVersion

class MainActivity : ComponentActivity() {
    /** Увеличивается в onResume, чтобы экран перечитал статусы разрешений. */
    private var resumes by mutableIntStateOf(0)

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()
        setContent {
            MaterialTheme {
                Surface(modifier = Modifier.fillMaxSize()) {
                    Root(resumes)
                }
            }
        }
    }

    override fun onResume() {
        super.onResume()
        Probe.uiVisible = true
        AppCore.openAsync(this)
        resumes++
    }

    override fun onPause() {
        Probe.uiVisible = false
        super.onPause()
    }
}

/**
 * Корень: ядро открывается лениво, затем онбординг или главный экран (4.2).
 * Экран замеров этапа 1 — по кнопке: замеры не зависят от аккаунта.
 */
@Composable
private fun Root(refresh: Int) {
    var showProbe by remember { mutableStateOf(false) }
    Column(modifier = Modifier.fillMaxSize().safeDrawingPadding()) {
        Row(modifier = Modifier.fillMaxWidth().padding(horizontal = 8.dp), horizontalArrangement = Arrangement.End) {
            TextButton(onClick = { showProbe = !showProbe }) { Text(if (showProbe) "Назад" else "Замеры") }
        }
        if (showProbe) {
            ProbeScreen(refresh)
            return@Column
        }
        when (val state = AppCore.state) {
            is AppCore.State.Open -> {
                LaunchedEffect(state) { AppModel.refresh(state.core, state.accountId) }
                when (val phase = AppModel.phase) {
                    AppModel.Phase.Loading -> CircularProgressIndicator(modifier = Modifier.padding(16.dp))
                    AppModel.Phase.Onboarding -> OnboardingScreen(state.core, state.accountId)
                    is AppModel.Phase.Ready -> HomeScreen(phase.nick, phase.server, phase.accountId)
                }
            }
            AppCore.State.Closed, AppCore.State.Opening -> CircularProgressIndicator(modifier = Modifier.padding(16.dp))
            is AppCore.State.Unavailable -> Text("Не удалось открыть данные: ${state.reason}", modifier = Modifier.padding(16.dp))
            is AppCore.State.Broken -> Column(modifier = Modifier.padding(16.dp)) {
                Text("Данные повреждены: ${state.reason}")
                val context = androidx.compose.ui.platform.LocalContext.current
                OutlinedButton(onClick = { AppCore.resetAsync(context) }) { Text("Сбросить локальные данные") }
            }
        }
    }
}

@Composable
private fun ProbeScreen(@Suppress("UNUSED_PARAMETER") refresh: Int) {
    val context = androidx.compose.ui.platform.LocalContext.current
    var token by remember { mutableStateOf("") }
    var tokenSaved by remember { mutableStateOf(Probe.hasToken) }

    val foregroundPermissions = rememberLauncherForActivityResult(
        ActivityResultContracts.RequestMultiplePermissions(),
    ) { }
    val backgroundPermission = rememberLauncherForActivityResult(ActivityResultContracts.RequestPermission()) { }

    fun start() {
        Probe.changeRunning(true)
        context.startForegroundService(
            Intent(context, LocationProbeService::class.java)
                .setAction(LocationProbeService.ACTION_START)
                .putExtra(LocationProbeService.EXTRA_FROM_UI, true),
        )
    }

    fun stop() {
        Probe.changeRunning(false)
        context.stopService(Intent(context, LocationProbeService::class.java))
        LocationProbeService.cancelHeartbeat(context)
    }

    Column(
        modifier = Modifier
            .fillMaxSize()
            .verticalScroll(rememberScrollState())
            .padding(16.dp),
        verticalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        Text("Staya · замер", style = MaterialTheme.typography.headlineSmall)

        Section("Замер")
        Probe.Strategy.entries.forEach { s ->
            Row(verticalAlignment = Alignment.CenterVertically) {
                RadioButton(
                    selected = Probe.strategy == s,
                    onClick = {
                        if (Probe.strategy != s) {
                            val wasRunning = Probe.running
                            if (wasRunning) stop()
                            Probe.select(s)
                            if (wasRunning) start()
                        }
                    },
                )
                Text(s.title)
            }
        }
        Row(verticalAlignment = Alignment.CenterVertically) {
            Text("Запущено", modifier = Modifier.weight(1f))
            Switch(checked = Probe.running, onCheckedChange = { if (it) start() else stop() })
        }
        Line("Режим", if (Probe.continuousOn) "непрерывно" else "экономно")
        Probe.lastError?.let { Line("Ошибка", it) }

        Section("Разрешения")
        val auth = Probe.auth(context)
        Line("Геолокация", authText(auth))
        Line("Точность", if (Probe.granted(context, Manifest.permission.ACCESS_FINE_LOCATION)) "точная" else "примерная")
        if (auth == ProbeRecord.Auth.DENIED) {
            Button(onClick = {
                foregroundPermissions.launch(
                    arrayOf(Manifest.permission.ACCESS_FINE_LOCATION, Manifest.permission.ACCESS_COARSE_LOCATION),
                )
            }) { Text("Разрешить геолокацию") }
        } else if (auth == ProbeRecord.Auth.WHEN_IN_USE) {
            // Android 11+: «Разрешить всегда» — отдельный шаг, система открывает настройки.
            Button(onClick = { backgroundPermission.launch(Manifest.permission.ACCESS_BACKGROUND_LOCATION) }) {
                Text("Геолокация «Разрешить всегда»")
            }
        }
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU &&
            !Probe.granted(context, Manifest.permission.POST_NOTIFICATIONS)
        ) {
            Button(onClick = { foregroundPermissions.launch(arrayOf(Manifest.permission.POST_NOTIFICATIONS)) }) {
                Text("Разрешить уведомление сервиса")
            }
        }
        val pm = context.getSystemService(PowerManager::class.java)
        val exempt = pm.isIgnoringBatteryOptimizations(context.packageName)
        Line("Оптимизация батареи", if (exempt) "отключена для Staya" else "включена")
        if (!exempt) {
            OutlinedButton(onClick = { requestBatteryExemption(context) }) { Text("Отключить оптимизацию батареи") }
        }
        Line("Энергосбережение", if (pm.isPowerSaveMode) "вкл" else "выкл")
        OutlinedButton(onClick = {
            context.startActivity(
                Intent(Settings.ACTION_APPLICATION_DETAILS_SETTINGS, "package:${context.packageName}".toUri()),
            )
        }) { Text("Настройки приложения") }
        Text(
            "Xiaomi/Poco: включить «Автозапуск», «Контроль активности» → «Нет ограничений». " +
                "Samsung: «Батарея» → «Неограниченно» и добавить в «Приложения, которые никогда не спят».",
            style = MaterialTheme.typography.bodySmall,
        )

        Section("Сборщик метрик")
        if (tokenSaved) Line("Токен", "сохранён в Keystore")
        OutlinedTextField(
            value = token,
            onValueChange = { token = it },
            label = { Text("Токен с сервера") },
            singleLine = true,
            visualTransformation = PasswordVisualTransformation(),
            keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Password),
            modifier = Modifier.fillMaxWidth(),
        )
        Button(enabled = token.trim().length >= 32, onClick = {
            tokenSaved = Probe.saveToken(token)
            token = ""
        }) { Text("Сохранить токен") }
        Button(enabled = tokenSaved, onClick = {
            Probe.record(ProbeRecord.Trigger.FOREGROUND, fromUi = true, force = true)
        }) { Text("Тестовая отправка") }

        Section("Счётчики")
        Line("Событий", Probe.eventsTotal.toString())
        Line("Отправлено", Probe.sentTotal.toString())
        Line("Сбоев", Probe.failedTotal.toString())
        Line("В очереди", Probe.queued.toString())
        Line("Последний ответ", Probe.lastStatus)
        Line(
            "Последнее событие",
            Probe.lastEventAt?.let { DateFormat.getTimeInstance(DateFormat.MEDIUM).format(Date(it)) } ?: "—",
        )
        OutlinedButton(onClick = { Probe.resetCounters() }) { Text("Сбросить счётчики") }

        Section("Окружение")
        Line("Ядро", coreVersion())
        Line("Аккаунт", coreStatus(AppCore.state))
        if (AppCore.state is AppCore.State.Broken) {
            OutlinedButton(onClick = { AppCore.resetAsync(context) }) { Text("Сбросить локальные данные") }
        }
        Line("Устройство", Probe.deviceId)
    }
}

/**
 * Прямой запрос исключения из оптимизации батареи. Lint (BatteryLife) против — это
 * правило Google Play; прототип ставится APK для замеров, а без исключения
 * производители телефонов убивают сервис, и замер теряет смысл.
 */
@SuppressLint("BatteryLife")
private fun requestBatteryExemption(context: android.content.Context) {
    context.startActivity(
        Intent(Settings.ACTION_REQUEST_IGNORE_BATTERY_OPTIMIZATIONS, "package:${context.packageName}".toUri()),
    )
}

private fun coreStatus(s: AppCore.State) = when (s) {
    AppCore.State.Closed, AppCore.State.Opening -> "открывается…"
    is AppCore.State.Open -> s.accountId.take(8) + "…"
    is AppCore.State.Unavailable -> "недоступно: ${s.reason}"
    is AppCore.State.Broken -> "база не читается: ${s.reason}"
}

private fun authText(a: ProbeRecord.Auth) = when (a) {
    ProbeRecord.Auth.ALWAYS -> "всегда"
    ProbeRecord.Auth.WHEN_IN_USE -> "только при использовании"
    ProbeRecord.Auth.DENIED -> "нет"
    ProbeRecord.Auth.RESTRICTED -> "ограничена"
    ProbeRecord.Auth.NOT_DETERMINED -> "не спрашивали"
}

@Composable
private fun Section(title: String) {
    HorizontalDivider(modifier = Modifier.padding(top = 8.dp))
    Text(title, style = MaterialTheme.typography.titleMedium)
}

@Composable
private fun Line(title: String, value: String) {
    Row(modifier = Modifier.fillMaxWidth()) {
        Text(title, modifier = Modifier.weight(1f), style = MaterialTheme.typography.bodyMedium)
        Text(value, style = MaterialTheme.typography.bodyMedium)
    }
}
