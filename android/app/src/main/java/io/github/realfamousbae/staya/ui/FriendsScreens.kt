package io.github.realfamousbae.staya.ui

import android.Manifest
import android.content.ClipboardManager
import android.content.Intent
import android.content.pm.PackageManager
import android.graphics.BitmapFactory
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.Image
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.BottomSheetScaffold
import androidx.compose.material3.Button
import androidx.compose.material3.RadioButton
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableLongStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.FilterQuality
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.delay
import io.github.realfamousbae.staya.location.SharingCard
import io.github.realfamousbae.staya.map.FriendsMap
import io.github.realfamousbae.staya.map.MapFocus
import io.github.realfamousbae.staya.map.locationStatus
import uniffi.staya_core.FriendView
import uniffi.staya_core.InviteMethod
import uniffi.staya_core.LocationKind
import uniffi.staya_core.Precision
import uniffi.staya_core.StayaCore

/** Главный экран: карта друзей (4.4), под ней список и добавление (4.3). */
@Composable
fun FriendsScreen(core: StayaCore, nick: String, server: String) {
    when (val s = FriendsModel.screen) {
        FriendsModel.Screen.Home -> FriendsHome(core, nick, server)
        is FriendsModel.Screen.ShowQr -> ShowQr(s.uri, s.expiresAt)
        is FriendsModel.Screen.ShareLink -> ShareLink(s.uri)
        FriendsModel.Screen.Scan -> Scan()
        is FriendsModel.Screen.Confirm -> Confirm(s)
        is FriendsModel.Screen.Friend -> FriendCard(s.friend, s.code)
    }
}

@Composable
private fun Back() {
    TextButton(onClick = { FriendsModel.screen = FriendsModel.Screen.Home }) { Text("← Назад") }
}

@Composable
private fun Message() {
    FriendsModel.message?.let {
        Text(it, color = MaterialTheme.colorScheme.primary)
        TextButton(onClick = { FriendsModel.message = null }) { Text("Понятно") }
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun FriendsHome(core: StayaCore, nick: String, server: String) {
    val context = LocalContext.current
    var focus by remember { mutableStateOf<MapFocus?>(null) }
    var now by remember { mutableLongStateOf(System.currentTimeMillis() / 1000) }
    LaunchedEffect(Unit) {
        while (true) {
            delay(60_000)
            now = System.currentTimeMillis() / 1000
        }
    }
    BottomSheetScaffold(
        sheetPeekHeight = 180.dp,
        sheetContent = {
            Column(
                modifier = Modifier.verticalScroll(rememberScrollState()).padding(horizontal = 16.dp).padding(bottom = 16.dp),
                verticalArrangement = Arrangement.spacedBy(8.dp),
            ) {
                Message()
                SharingCard(core)
                HorizontalDivider()
                Text("Друзья", style = MaterialTheme.typography.titleMedium)
                if (FriendsModel.friends.isEmpty()) {
                    Text("Пока никого. Покажи QR-код другу рядом или отправь ссылку.", style = MaterialTheme.typography.bodyMedium)
                }
                FriendsModel.friends.forEach { f -> FriendRow(f, now) { focus = MapFocus(f.accountId) } }

                HorizontalDivider()
                Text("Добавить друга", style = MaterialTheme.typography.titleMedium)
                Button(onClick = { FriendsModel.invite(InviteMethod.QR) }) { Text("Показать мой QR-код") }
                OutlinedButton(onClick = { FriendsModel.invite(InviteMethod.LINK) }) { Text("Отправить ссылку") }
                OutlinedButton(onClick = { FriendsModel.screen = FriendsModel.Screen.Scan }) { Text("Сканировать QR-код друга") }
                OutlinedButton(onClick = {
                    val text = context.getSystemService(ClipboardManager::class.java).primaryClip
                        ?.getItemAt(0)?.coerceToText(context)?.toString()
                    val link = DeepLink.parse(text)
                    if (link == null) {
                        FriendsModel.message = "В буфере нет ссылки staya://. Скопируй приглашение целиком."
                    } else {
                        FriendsModel.open(link)
                    }
                }) { Text("Вставить ссылку друга") }

                HorizontalDivider()
                Text("$nick · сервер $server", style = MaterialTheme.typography.bodySmall)
            }
        },
    ) { padding ->
        FriendsMap(core, server, FriendsModel.friends, focus, modifier = Modifier.fillMaxSize().padding(padding))
    }
}

@Composable
private fun FriendRow(f: FriendView, now: Long, onShow: () -> Unit) {
    val visible = f.location?.let { it.kind != LocationKind.HIDDEN } == true
    Row(
        modifier = Modifier.fillMaxWidth().clickable(enabled = visible, onClick = onShow).padding(vertical = 6.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        f.avatar?.takeIf { it.isNotEmpty() }?.let { bytes ->
            BitmapFactory.decodeByteArray(bytes, 0, bytes.size)?.let {
                Image(it.asImageBitmap(), contentDescription = null, modifier = Modifier.size(40.dp).clip(CircleShape))
                Spacer(Modifier.width(12.dp))
            }
        }
        Column(modifier = Modifier.weight(1f)) {
            Text(f.nick ?: "Без имени")
            val status = when {
                !f.active -> "ждём ответа"
                else -> locationStatus(f, now) ?: "пока нет позиции"
            }
            Text(status, style = MaterialTheme.typography.bodySmall)
            if (f.active && !f.verified) {
                Text("не проверен — сверьте код безопасности", style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.error)
            }
        }
        TextButton(onClick = { FriendsModel.showFriend(f) }) { Text("Ещё") }
    }
}

@Composable
private fun ShowQr(uri: String, expiresAt: Long) {
    var left by remember { mutableLongStateOf(expiresAt - System.currentTimeMillis()) }
    LaunchedEffect(expiresAt) {
        while (left > 0) {
            delay(1000)
            left = expiresAt - System.currentTimeMillis()
        }
    }
    val bitmap = remember(uri) { Qr.bitmap(uri).asImageBitmap() }
    Column(modifier = Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
        Back()
        Text("Покажи код другу рядом", style = MaterialTheme.typography.titleLarge)
        if (left > 0) {
            Image(
                bitmap,
                contentDescription = "QR-код приглашения",
                filterQuality = FilterQuality.None,
                modifier = Modifier.fillMaxWidth().aspectRatio(1f),
            )
            Text("Действует ещё ${left / 60_000}:${"%02d".format((left / 1000) % 60)}. Не закрывай приложение, пока друг не отсканирует.")
        } else {
            Text("Код истёк.")
        }
        OutlinedButton(onClick = { FriendsModel.invite(InviteMethod.QR) }) { Text("Новый код") }
        Text(
            "Добавленный по QR при встрече друг сразу считается проверенным.",
            style = MaterialTheme.typography.bodySmall,
        )
    }
}

@Composable
private fun ShareLink(uri: String) {
    val context = LocalContext.current
    Column(modifier = Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
        Back()
        Text("Ссылка-приглашение", style = MaterialTheme.typography.titleLarge)
        Text(
            "Ссылка действует 24 часа. Открой Staya в течение суток, чтобы принять ответ друга. " +
                "Добавленный по ссылке друг будет «не проверен», пока вы не сверите код безопасности.",
        )
        Button(onClick = {
            val send = Intent(Intent.ACTION_SEND).setType("text/plain").putExtra(Intent.EXTRA_TEXT, uri)
            context.startActivity(Intent.createChooser(send, "Отправить приглашение"))
        }) { Text("Отправить") }
    }
}

@Composable
private fun Scan() {
    val context = LocalContext.current
    var granted by remember {
        androidx.compose.runtime.mutableStateOf(
            context.checkSelfPermission(Manifest.permission.CAMERA) == PackageManager.PERMISSION_GRANTED,
        )
    }
    val ask = rememberLauncherForActivityResult(ActivityResultContracts.RequestPermission()) { granted = it }
    Column(modifier = Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
        Back()
        Text("Наведи камеру на QR-код друга", style = MaterialTheme.typography.titleLarge)
        if (granted) {
            QrScanner(Modifier.fillMaxWidth().aspectRatio(1f)) { FriendsModel.open(it) }
        } else {
            Text("Нужен доступ к камере. Кадры никуда не сохраняются и не отправляются.")
            Button(onClick = { ask.launch(Manifest.permission.CAMERA) }) { Text("Разрешить камеру") }
        }
    }
}

@Composable
private fun Confirm(s: FriendsModel.Screen.Confirm) {
    Column(modifier = Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
        Back()
        Text("Добавить друга?", style = MaterialTheme.typography.titleLarge)
        Text("Этот человек будет видеть, где ты, пока ты не скроешь позицию или не удалишь его.")
        Text("Сервер: ${s.info.server}", style = MaterialTheme.typography.bodySmall)
        Text(
            if (s.info.method == InviteMethod.QR) {
                "Код отсканирован при встрече — друг будет проверенным."
            } else {
                "Приглашение по ссылке — сверьте потом код безопасности."
            },
            style = MaterialTheme.typography.bodySmall,
        )
        Message()
        Button(enabled = !FriendsModel.busy, onClick = { FriendsModel.accept(s.uri) }) { Text("Добавить") }
        OutlinedButton(onClick = { FriendsModel.screen = FriendsModel.Screen.Home }) { Text("Отмена") }
    }
}

@Composable
private fun FriendCard(initial: FriendView, code: String) {
    // Свежая версия из списка: точность и статус меняются, пока карточка открыта.
    val friend = FriendsModel.friends.firstOrNull { it.accountId == initial.accountId } ?: initial
    var confirmRemove by remember { mutableStateOf(false) }
    Column(
        modifier = Modifier.verticalScroll(rememberScrollState()).padding(16.dp),
        verticalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        Back()
        Text(friend.nick ?: "Друг", style = MaterialTheme.typography.titleLarge)
        Message()

        if (friend.active) {
            Text("Что видит этот друг", style = MaterialTheme.typography.titleMedium)
            listOf(
                Precision.EXACT to "Точную позицию",
                Precision.APPROX to "Примерно (район ~1 км)",
                Precision.HIDDEN to "Ничего — «скрыл(а) позицию»",
            ).forEach { (p, title) ->
                Row(
                    modifier = Modifier.fillMaxWidth().clickable { FriendsModel.setPrecision(friend, p) },
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    RadioButton(selected = friend.precision == p, onClick = { FriendsModel.setPrecision(friend, p) })
                    Text(title)
                }
            }
            HorizontalDivider()
        }

        Text("Код безопасности", style = MaterialTheme.typography.titleMedium)
        Text(code, style = MaterialTheme.typography.headlineSmall)
        Text(
            "Сравните код при встрече или голосом. Совпадает — значит, между вами никого нет. " +
                "Если код другой — не нажимай «Совпадает» и удали этого друга.",
        )
        if (friend.verified) {
            Text("Уже проверен.", color = MaterialTheme.colorScheme.primary)
        } else if (friend.active) {
            Button(onClick = { FriendsModel.markVerified(friend) }) { Text("Совпадает") }
        }

        HorizontalDivider()
        OutlinedButton(onClick = { confirmRemove = true }) {
            Text("Удалить друга", color = MaterialTheme.colorScheme.error)
        }
    }
    if (confirmRemove) {
        AlertDialog(
            onDismissRequest = { confirmRemove = false },
            title = { Text("Удалить ${friend.nick ?: "друга"}?") },
            text = { Text("Он сразу перестанет видеть твою позицию, а ты — его. Чтобы снова дружить, придётся добавить друг друга заново.") },
            confirmButton = {
                TextButton(onClick = {
                    confirmRemove = false
                    FriendsModel.remove(friend)
                }) { Text("Удалить", color = MaterialTheme.colorScheme.error) }
            },
            dismissButton = { TextButton(onClick = { confirmRemove = false }) { Text("Отмена") } },
        )
    }
}
