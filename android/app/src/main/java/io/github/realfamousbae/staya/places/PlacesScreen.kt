package io.github.realfamousbae.staya.places

import android.Manifest
import android.content.pm.PackageManager
import android.os.Build
import android.os.Handler
import android.os.Looper
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.layout.*
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.runtime.*
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.dp
import io.github.realfamousbae.staya.map.FriendsMap
import io.github.realfamousbae.staya.net.Net
import io.github.realfamousbae.staya.ui.*
import uniffi.staya_core.PlaceView
import uniffi.staya_core.StayaCore

@Composable
fun PlacesScreen(core: StayaCore, server: String) {
    val context = LocalContext.current
    val main = remember { Handler(Looper.getMainLooper()) }
    var places by remember { mutableStateOf<List<PlaceView>>(emptyList()) }
    var name by remember { mutableStateOf("") }
    var radius by remember { mutableStateOf("250") }
    var picked by remember { mutableStateOf<Pair<Int, Int>?>(null) }
    var friend by remember { mutableStateOf<String?>(null) }
    var busy by remember { mutableStateOf(false) }
    var message by remember { mutableStateOf<String?>(null) }
    val friends = FriendsModel.friends.filter { it.active }
    val permission = rememberLauncherForActivityResult(ActivityResultContracts.RequestPermission()) { granted ->
        if (!granted) message = "Место сохранено. Разреши уведомления в настройках телефона, чтобы получать сообщения."
    }
    LaunchedEffect(core) {
        Net.worker.execute {
            val result = runCatching { core.listPlaces() }
            main.post { places = result.getOrDefault(emptyList()); if (result.isFailure) message = "Не удалось прочитать места." }
        }
    }
    DarkScreen("места", onBack = { FriendsModel.screen = FriendsModel.Screen.Settings }) {
        Text("Места хранятся только на этом телефоне. Уведомления появятся, когда придёт свежая позиция друга.", color = Ink.mist)
        message?.let { Text(it, color = Ink.mist) }
        places.forEach { place ->
            ListRow(place.name, "${friends.find { it.accountId == place.friend }?.nick ?: "Друг"} · ${place.radiusM} м")
            SoftButton(onClick = {
                busy = true
                Net.worker.execute {
                    val result = runCatching { core.removePlace(place.id); core.listPlaces() }
                    main.post { result.onSuccess { places = it; PlaceNotifications.cancel(place.id) }; if (result.isFailure) message = "Не удалось удалить место."; busy = false }
                }
            }, enabled = !busy) { Text("Удалить место") }
        }
        SectionTitle("новое место")
        if (friends.isEmpty()) Text("Сначала добавь друга и дождись подтверждения.", color = Ink.mist)
        friends.forEach { f ->
            ListRow(f.nick ?: "Друг", if (friend == f.accountId) "Выбран" else "Выбрать друга", enabled = !busy) { friend = f.accountId }
        }
        Text("Нажми и удерживай точку на карте. Затем укажи название и радиус.", color = Ink.mist)
        FriendsMap(core, server, emptyList(), null, Modifier.fillMaxWidth().height(280.dp),
            onPickPlace = { lat, lon -> picked = lat to lon }, pickedPlace = picked)
        Text(if (picked == null) "Точка пока не выбрана" else "Точка выбрана", color = Ink.mist)
        OutlinedTextField(name, { name = it }, label = { Text("Название места") }, enabled = !busy, modifier = Modifier.fillMaxWidth())
        OutlinedTextField(radius, { radius = it }, label = { Text("Радиус, м (100–5000)") }, enabled = !busy, modifier = Modifier.fillMaxWidth())
        val meters = radius.toIntOrNull()
        val valid = friend != null && picked != null && name.isNotBlank() && name.toByteArray(Charsets.UTF_8).size <= 64 && meters != null && meters in 100..5000
        Pill("Сохранить место", {
            val target = friend ?: return@Pill
            val point = picked ?: return@Pill
            val distance = meters ?: return@Pill
            busy = true
            val label = name
            Net.worker.execute {
                val result = runCatching { core.addPlace(target, label, point.first, point.second, distance.toUShort()); core.listPlaces() }
                main.post {
                    result.onSuccess {
                        places = it; name = ""; picked = null; message = "Место сохранено."
                        if (Build.VERSION.SDK_INT >= 33 && context.checkSelfPermission(Manifest.permission.POST_NOTIFICATIONS) != PackageManager.PERMISSION_GRANTED) permission.launch(Manifest.permission.POST_NOTIFICATIONS)
                    }
                    if (result.isFailure) message = "Не удалось сохранить место. Проверь название и радиус."
                    busy = false
                }
            }
        }, enabled = valid && !busy)
    }
}
