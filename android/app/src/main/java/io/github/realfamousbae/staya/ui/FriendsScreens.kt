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
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Button
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableLongStateOf
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
import uniffi.staya_core.FriendView
import uniffi.staya_core.InviteMethod

/** Главный экран: друзья и добавление (4.3). Карта — 4.4. */
@Composable
fun FriendsScreen(nick: String, server: String) {
    when (val s = FriendsModel.screen) {
        FriendsModel.Screen.Home -> FriendsHome(nick, server)
        is FriendsModel.Screen.ShowQr -> ShowQr(s.uri, s.expiresAt)
        is FriendsModel.Screen.ShareLink -> ShareLink(s.uri)
        FriendsModel.Screen.Scan -> Scan()
        is FriendsModel.Screen.Confirm -> Confirm(s)
        is FriendsModel.Screen.Safety -> Safety(s.friend, s.code)
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

@Composable
private fun FriendsHome(nick: String, server: String) {
    val context = LocalContext.current
    Column(
        modifier = Modifier.verticalScroll(rememberScrollState()).padding(16.dp),
        verticalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        Text(nick, style = MaterialTheme.typography.headlineMedium)
        Text("Сервер: $server", style = MaterialTheme.typography.bodySmall)
        Message()

        Text("Друзья", style = MaterialTheme.typography.titleMedium)
        if (FriendsModel.friends.isEmpty()) {
            Text("Пока никого. Покажи QR-код другу рядом или отправь ссылку.", style = MaterialTheme.typography.bodyMedium)
        }
        FriendsModel.friends.forEach { FriendRow(it) }

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
    }
}

@Composable
private fun FriendRow(f: FriendView) {
    Row(
        modifier = Modifier.fillMaxWidth().clickable { FriendsModel.showSafety(f) }.padding(vertical = 6.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        f.avatar?.takeIf { it.isNotEmpty() }?.let { bytes ->
            BitmapFactory.decodeByteArray(bytes, 0, bytes.size)?.let {
                Image(it.asImageBitmap(), contentDescription = null, modifier = Modifier.size(40.dp).clip(CircleShape))
                Spacer(Modifier.width(12.dp))
            }
        }
        Column {
            Text(f.nick ?: "Без имени")
            val status = when {
                !f.active -> "ждём ответа"
                f.verified -> "проверен"
                else -> "не проверен — сверьте код безопасности"
            }
            Text(status, style = MaterialTheme.typography.bodySmall)
        }
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
private fun Safety(friend: FriendView, code: String) {
    Column(modifier = Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
        Back()
        Text(friend.nick ?: "Друг", style = MaterialTheme.typography.titleLarge)
        Text("Код безопасности", style = MaterialTheme.typography.titleMedium)
        Text(code, style = MaterialTheme.typography.headlineSmall)
        Text(
            "Сравните код при встрече или голосом. Совпадает — значит, между вами никого нет. " +
                "Если код другой — не нажимай «Совпадает» и удали этого друга.",
        )
        if (friend.verified) {
            Text("Уже проверен.", color = MaterialTheme.colorScheme.primary)
        } else {
            Button(onClick = { FriendsModel.markVerified(friend) }) { Text("Совпадает") }
        }
    }
}
