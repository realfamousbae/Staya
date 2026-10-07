package io.github.realfamousbae.staya.ui

import android.Manifest
import android.content.ClipboardManager
import android.content.Context
import android.content.pm.PackageManager
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.PickVisualMediaRequest
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.Image
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

/**
 * Первый запуск (задачи 4.2, 4.9b): откуда сервер, ник и аватар. Ключи уже созданы на
 * устройстве. Проще всего — отсканировать QR друга при встрече: в нём сервер, отпечаток
 * его ключа и код регистрации, а друг сразу становится проверенным (protocol §5.1).
 */
@Composable
fun OnboardingScreen(core: StayaCore, accountId: String) {
    val context = LocalContext.current
    var link by remember { mutableStateOf("") }
    // Ссылка получена камерой приложения — только тогда QR даёт «проверено» (§5.1).
    var scanned by remember { mutableStateOf(false) }
    var scanning by remember { mutableStateOf(false) }
    var cameraGranted by remember {
        mutableStateOf(context.checkSelfPermission(Manifest.permission.CAMERA) == PackageManager.PERMISSION_GRANTED)
    }
    val askCamera = rememberLauncherForActivityResult(ActivityResultContracts.RequestPermission()) {
        cameraGranted = it
        scanning = it
    }
    // Ссылка, открытая извне до создания аккаунта, — в поле (без автоматического принятия).
    androidx.compose.runtime.LaunchedEffect(AppModel.pendingLink) {
        AppModel.pendingLink?.let {
            link = it.uri
            scanned = false
            AppModel.pendingLink = null
        }
    }
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

        Text("Приглашение друга", style = MaterialTheme.typography.titleMedium)
        Text(
            "Рядом с другом — отсканируй QR из его Staya. Далеко — открой ссылку, которую он прислал, " +
                "или вставь её сюда. Сервер и всё нужное для входа уже в приглашении.",
            style = MaterialTheme.typography.bodyMedium,
        )
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            Button(onClick = {
                if (cameraGranted) scanning = !scanning else askCamera.launch(Manifest.permission.CAMERA)
            }) { Text(if (scanning) "Закрыть камеру" else "Сканировать QR") }
            OutlinedButton(onClick = {
                val clip = context.getSystemService(ClipboardManager::class.java).primaryClip
                clip?.getItemAt(0)?.coerceToText(context)?.toString()?.let {
                    link = it
                    scanned = false
                    scanning = false
                }
            }) { Text("Вставить") }
        }
        if (scanning && cameraGranted) {
            QrScanner(Modifier.fillMaxWidth().aspectRatio(1f)) {
                link = it.uri
                scanned = true
                scanning = false
            }
            Text("Кадры никуда не сохраняются и не отправляются.", style = MaterialTheme.typography.bodySmall)
        }
        OutlinedTextField(
            value = link,
            onValueChange = {
                link = it
                scanned = false
            },
            placeholder = { Text("Ссылка-приглашение или ссылка на сервер") },
            modifier = Modifier.fillMaxWidth(),
            maxLines = 4,
        )
        when (DeepLink.parse(link)) {
            is DeepLink.Invite -> Text(
                if (scanned) {
                    "QR друга отсканирован: после создания аккаунта вы станете друзьями — сразу проверенными. Он будет видеть, где ты."
                } else {
                    "Это приглашение: после создания аккаунта пригласивший станет твоим другом и будет видеть, где ты."
                },
                color = MaterialTheme.colorScheme.primary,
            )
            is DeepLink.Server -> Text("Это ссылка на сервер — друзей добавишь потом.", color = MaterialTheme.colorScheme.primary)
            null -> {}
        }

        TextButton(onClick = { advanced = !advanced }) { Text(if (advanced) "Скрыть дополнительное" else "Дополнительно") }
        // Поле кода — сразу, когда сервер его попросил; иначе в «Дополнительно».
        if (AppModel.needCode) CodeField(code) { code = it }
        if (advanced) {
            OutlinedTextField(
                value = host,
                onValueChange = { host = it },
                label = { Text("Сервер (host или host:port)") },
                singleLine = true,
                keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Uri),
                modifier = Modifier.fillMaxWidth(),
            )
            if (!AppModel.needCode) CodeField(code) { code = it }
            Text(
                "Код нужен только для закрытого сервера, если его нет в ссылке. Без отпечатка ключа сервер запоминается при первом подключении.",
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
            onClick = { AppModel.createAccount(core, accountId, link, host, code, nick, avatar, scanned) },
        ) {
            if (AppModel.busy) {
                CircularProgressIndicator(modifier = Modifier.size(18.dp), strokeWidth = 2.dp)
                Spacer(Modifier.width(8.dp))
            }
            Text("Создать аккаунт")
        }
    }
}

@Composable
private fun CodeField(code: String, onChange: (String) -> Unit) {
    OutlinedTextField(
        value = code,
        onValueChange = onChange,
        label = { Text("Код регистрации на сервере") },
        singleLine = true,
        visualTransformation = PasswordVisualTransformation(),
        keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Password),
        modifier = Modifier.fillMaxWidth(),
    )
}
