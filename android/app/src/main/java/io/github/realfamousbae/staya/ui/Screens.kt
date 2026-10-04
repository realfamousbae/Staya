package io.github.realfamousbae.staya.ui

import android.content.ClipboardManager
import android.content.Context
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.PickVisualMediaRequest
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.Image
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Button
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.unit.dp
import android.graphics.BitmapFactory
import uniffi.staya_core.StayaCore

/** Первый запуск (задача 4.2): откуда сервер, ник и аватар. Ключи уже созданы на устройстве. */
@Composable
fun OnboardingScreen(core: StayaCore, accountId: String) {
    val context = LocalContext.current
    var link by remember { mutableStateOf("") }
    var host by remember { mutableStateOf("") }
    var code by remember { mutableStateOf("") }
    var nick by remember { mutableStateOf("") }
    var avatar by remember { mutableStateOf<ByteArray?>(null) }
    var avatarError by remember { mutableStateOf(false) }
    var advanced by remember { mutableStateOf(false) }
    val pick = rememberLauncherForActivityResult(ActivityResultContracts.PickVisualMedia()) { uri ->
        if (uri != null) {
            avatar = Avatar.encode(context, uri)
            avatarError = avatar == null
        }
    }
    val nickTooLong = nick.toByteArray(Charsets.UTF_8).size > 64
    val canCreate = nick.isNotBlank() && !nickTooLong && (link.isNotBlank() || host.isNotBlank()) && !AppModel.busy

    Column(
        modifier = Modifier.verticalScroll(rememberScrollState()).padding(16.dp),
        verticalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        Text("Staya", style = MaterialTheme.typography.headlineMedium)
        Text("Staya показывает друзьям, где ты, — и никому больше. Координаты шифруются на телефоне, сервер их не видит.")

        Text("Приглашение или ссылка на сервер", style = MaterialTheme.typography.titleMedium)
        OutlinedTextField(
            value = link,
            onValueChange = { link = it },
            placeholder = { Text("staya://…") },
            modifier = Modifier.fillMaxWidth(),
            maxLines = 4,
        )
        Row(verticalAlignment = Alignment.CenterVertically) {
            OutlinedButton(onClick = {
                val clip = context.getSystemService(ClipboardManager::class.java).primaryClip
                clip?.getItemAt(0)?.coerceToText(context)?.toString()?.let { link = it }
            }) { Text("Вставить") }
        }
        Text(
            "Приглашение присылает друг. Ссылку на сервер — его владелец.",
            style = MaterialTheme.typography.bodySmall,
        )

        TextButton(onClick = { advanced = !advanced }) { Text(if (advanced) "Скрыть дополнительное" else "Дополнительно") }
        if (advanced) {
            OutlinedTextField(
                value = host,
                onValueChange = { host = it },
                label = { Text("Сервер (host или host:port)") },
                singleLine = true,
                keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Uri),
                modifier = Modifier.fillMaxWidth(),
            )
            OutlinedTextField(
                value = code,
                onValueChange = { code = it },
                label = { Text("Код приглашения на регистрацию") },
                singleLine = true,
                visualTransformation = PasswordVisualTransformation(),
                keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Password),
                modifier = Modifier.fillMaxWidth(),
            )
            Text(
                "Код нужен только для закрытого сервера. Без отпечатка ключа сервер запоминается при первом подключении.",
                style = MaterialTheme.typography.bodySmall,
            )
        }

        Text("Профиль", style = MaterialTheme.typography.titleMedium)
        OutlinedTextField(
            value = nick,
            onValueChange = { nick = it },
            label = { Text("Ник") },
            singleLine = true,
            isError = nickTooLong,
            supportingText = { if (nickTooLong) Text("Слишком длинный ник") },
            modifier = Modifier.fillMaxWidth(),
        )
        Row(verticalAlignment = Alignment.CenterVertically) {
            avatar?.let { bytes ->
                BitmapFactory.decodeByteArray(bytes, 0, bytes.size)?.let {
                    Image(it.asImageBitmap(), contentDescription = null, modifier = Modifier.size(44.dp).clip(CircleShape))
                    Spacer(Modifier.width(12.dp))
                }
            }
            OutlinedButton(onClick = {
                pick.launch(PickVisualMediaRequest(ActivityResultContracts.PickVisualMedia.ImageOnly))
            }) { Text(if (avatar == null) "Выбрать аватар" else "Сменить аватар") }
        }
        if (avatarError) {
            Text("Не получилось уменьшить фото до 8 КБ — выбери другое", color = MaterialTheme.colorScheme.error)
        }

        AppModel.error?.let { Text(it, color = MaterialTheme.colorScheme.error) }

        Button(
            enabled = canCreate,
            onClick = { AppModel.createAccount(core, accountId, link, host, code, nick, avatar) },
        ) {
            if (AppModel.busy) {
                CircularProgressIndicator(modifier = Modifier.size(18.dp), strokeWidth = 2.dp)
                Spacer(Modifier.width(8.dp))
            }
            Text("Создать аккаунт")
        }
    }
}

/** Главный экран (пока заготовка): карта и друзья — задачи 4.3–4.4. */
@Composable
fun HomeScreen(nick: String, server: String, accountId: String) {
    Column(modifier = Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
        Text(nick, style = MaterialTheme.typography.headlineMedium)
        Text("Сервер: $server")
        Text("Аккаунт: ${accountId.take(8)}…")
        Text("Карта и друзья появятся в следующих версиях.", style = MaterialTheme.typography.bodySmall)
    }
}

