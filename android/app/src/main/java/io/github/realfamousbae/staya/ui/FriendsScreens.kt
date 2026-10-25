package io.github.realfamousbae.staya.ui

import android.content.ClipboardManager
import android.content.Intent
import android.graphics.BitmapFactory
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.asPaddingValues
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.safeDrawing
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.RadioButton
import androidx.compose.material3.RadioButtonDefaults
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
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.FilterQuality
import androidx.compose.ui.graphics.Shadow
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import io.github.realfamousbae.staya.R
import io.github.realfamousbae.staya.location.LocationShare
import io.github.realfamousbae.staya.location.SharingCard
import io.github.realfamousbae.staya.map.FriendsMap
import io.github.realfamousbae.staya.map.MapFocus
import io.github.realfamousbae.staya.map.locationStatus
import kotlinx.coroutines.delay
import uniffi.staya_core.FriendView
import uniffi.staya_core.InviteMethod
import uniffi.staya_core.LocationKind
import uniffi.staya_core.Precision
import uniffi.staya_core.StayaCore

/** Главный экран (5.2): карта во весь экран, хром поверх неё, остальное — шторки и тёмные экраны. */
@Composable
fun FriendsScreen(core: StayaCore, nick: String, server: String) {
    when (val s = FriendsModel.screen) {
        FriendsModel.Screen.Home -> FriendsHome(core, server)
        FriendsModel.Screen.Settings -> SettingsScreen(core, nick, server)
        is FriendsModel.Screen.ShowQr -> ShowQr(s.uri, s.expiresAt)
        is FriendsModel.Screen.ShareLink -> ShareLink(s.uri)
        FriendsModel.Screen.Scan -> Scan()
        is FriendsModel.Screen.Confirm -> Confirm(s)
        is FriendsModel.Screen.ConfirmServerCode -> ConfirmServerCode(s)
        is FriendsModel.Screen.Friend -> FriendCard(s.friend, s.code)
    }
}

private fun home() {
    FriendsModel.screen = FriendsModel.Screen.Home
}

/** Сообщение модели: карточка с «Понятно». */
@Composable
private fun Message() {
    FriendsModel.message?.let {
        Column(
            modifier = Modifier.fillMaxWidth().clip(RoundedCornerShape(18.dp)).background(Ink.ink2).padding(16.dp),
            verticalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            Text(it, style = MaterialTheme.typography.bodyMedium, color = Ink.paper)
            Text(
                "Понятно",
                style = MaterialTheme.typography.labelLarge,
                color = Ink.mint,
                modifier = Modifier.clickable { FriendsModel.message = null }.padding(vertical = 4.dp),
            )
        }
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun FriendsHome(core: StayaCore, server: String) {
    val context = LocalContext.current
    val dark = isSystemInDarkTheme()
    var focus by remember { mutableStateOf<MapFocus?>(null) }
    var friendsSheet by remember { mutableStateOf(false) }
    var addSheet by remember { mutableStateOf(false) }
    var now by remember { mutableLongStateOf(System.currentTimeMillis() / 1000) }
    LaunchedEffect(Unit) {
        while (true) {
            delay(60_000)
            now = System.currentTimeMillis() / 1000
        }
    }
    val sharing = remember(FriendsModel.friends, friendsSheet, addSheet) { LocationShare.isEnabled(context) }
    val frozen = remember(sharing, friendsSheet, addSheet) { sharing && LocationShare.isFrozen(core) }
    val friends = FriendsModel.friends
    val onMap = friends.count { f -> f.location?.let { it.kind != LocationKind.HIDDEN } == true }
    val waiting = friends.count { !it.active }

    val insets = WindowInsets.safeDrawing.asPaddingValues()
    val textColor = if (dark) Ink.paper else Ink.ink
    val halo = if (dark) Ink.ink else Ink.paper

    Box(modifier = Modifier.fillMaxSize()) {
        FriendsMap(core, server, friends, focus, modifier = Modifier.fillMaxSize())

        // Заголовок-состояние: главное, что должен видеть владелец, — виден ли он сейчас.
        // Нажатие ведёт в настройки, где передача включается.
        Column(
            modifier = Modifier
                .align(Alignment.TopStart)
                .padding(insets)
                .padding(start = 20.dp, top = 16.dp, end = 84.dp)
                .clickable { FriendsModel.screen = FriendsModel.Screen.Settings },
        ) {
            Text(
                when {
                    !sharing -> "позиция скрыта"
                    frozen -> "заморожено здесь"
                    else -> "делюсь позицией"
                },
                style = MaterialTheme.typography.displaySmall.copy(shadow = Shadow(halo, blurRadius = 12f)),
                color = textColor,
            )
            Text(
                when {
                    friends.isEmpty() -> "Друзей пока нет"
                    onMap == 0 && waiting > 0 -> "Ждём ответа: $waiting"
                    onMap == 0 -> "Никого на карте"
                    waiting > 0 -> "На карте: $onMap, ждём ответа: $waiting"
                    else -> "На карте: $onMap"
                },
                style = MaterialTheme.typography.bodyMedium.copy(shadow = Shadow(halo, blurRadius = 10f)),
                color = textColor,
            )
            if (!sharing) {
                Spacer(Modifier.height(12.dp))
                Pill("Делиться позицией", { FriendsModel.screen = FriendsModel.Screen.Settings }, modifier = Modifier.width(220.dp))
            }
            FriendsModel.message?.let {
                Spacer(Modifier.height(12.dp))
                Message()
            }
        }

        RoundButton(
            R.drawable.ic_settings,
            "Настройки",
            { FriendsModel.screen = FriendsModel.Screen.Settings },
            modifier = Modifier.align(Alignment.TopEnd).padding(insets).padding(end = 20.dp, top = 16.dp),
            size = 48,
        )

        Row(
            modifier = Modifier.align(Alignment.BottomCenter).padding(insets).padding(bottom = 24.dp),
            horizontalArrangement = Arrangement.spacedBy(16.dp),
        ) {
            RoundButton(R.drawable.ic_people, "Друзья", { friendsSheet = true })
            RoundButton(R.drawable.ic_locate, "Показать всех", { focus = MapFocus(null) })
            RoundButton(R.drawable.ic_add, "Добавить друга", { addSheet = true })
        }
    }

    if (friendsSheet) {
        ModalBottomSheet(onDismissRequest = { friendsSheet = false }, containerColor = Ink.ink2, contentColor = Ink.paper) {
            Column(modifier = Modifier.padding(horizontal = 20.dp).padding(bottom = 24.dp), verticalArrangement = Arrangement.spacedBy(4.dp)) {
                Text("друзья", style = MaterialTheme.typography.titleLarge, modifier = Modifier.padding(bottom = 8.dp))
                if (friends.isEmpty()) {
                    Text("Пока никого. Покажи QR-код другу рядом или отправь ссылку.", style = MaterialTheme.typography.bodyMedium, color = Ink.mist)
                    Spacer(Modifier.height(12.dp))
                    Pill("Показать мой QR-код", { friendsSheet = false; FriendsModel.invite(InviteMethod.QR) })
                }
                friends.forEach { f ->
                    FriendRow(
                        f,
                        now,
                        onShow = {
                            friendsSheet = false
                            focus = MapFocus(f.accountId)
                        },
                        onMore = {
                            friendsSheet = false
                            FriendsModel.showFriend(f)
                        },
                    )
                }
            }
        }
    }

    if (addSheet) {
        ModalBottomSheet(onDismissRequest = { addSheet = false }, containerColor = Ink.ink2, contentColor = Ink.paper) {
            Column(modifier = Modifier.padding(horizontal = 20.dp).padding(bottom = 24.dp), verticalArrangement = Arrangement.spacedBy(4.dp)) {
                Text("добавить друга", style = MaterialTheme.typography.titleLarge, modifier = Modifier.padding(bottom = 8.dp))
                Pill("Показать мой QR-код", { addSheet = false; FriendsModel.invite(InviteMethod.QR) })
                Spacer(Modifier.height(8.dp))
                ListRow("Отправить ссылку", "Действует сутки, откроется из мессенджера") {
                    addSheet = false
                    FriendsModel.invite(InviteMethod.LINK)
                }
                ListRow("Сканировать QR-код друга", "Рядом с другом — сразу проверенный") {
                    addSheet = false
                    FriendsModel.screen = FriendsModel.Screen.Scan
                }
                ListRow("Вставить ссылку друга", "Из буфера обмена") {
                    addSheet = false
                    val text = context.getSystemService(ClipboardManager::class.java).primaryClip
                        ?.getItemAt(0)?.coerceToText(context)?.toString()
                    val link = DeepLink.parse(text)
                    if (link == null) {
                        FriendsModel.message = "В буфере нет ссылки Staya. Скопируй приглашение целиком."
                    } else {
                        FriendsModel.open(link)
                    }
                }
            }
        }
    }
}

/** Аватар в круге; без картинки — первая буква ника на мятном. */
@Composable
fun AvatarCircle(f: FriendView, size: Int = 44) {
    val bitmap = remember(f.avatar) { f.avatar?.takeIf { it.isNotEmpty() }?.let { BitmapFactory.decodeByteArray(it, 0, it.size) } }
    if (bitmap != null) {
        Image(bitmap.asImageBitmap(), contentDescription = null, modifier = Modifier.size(size.dp).clip(CircleShape))
    } else {
        Box(modifier = Modifier.size(size.dp).clip(CircleShape).background(Ink.accent), contentAlignment = Alignment.Center) {
            Text(f.nick?.trim()?.firstOrNull()?.uppercase() ?: "?", style = MaterialTheme.typography.titleMedium, color = Ink.ink)
        }
    }
}

@Composable
private fun FriendRow(f: FriendView, now: Long, onShow: () -> Unit, onMore: () -> Unit) {
    val visible = f.location?.let { it.kind != LocationKind.HIDDEN } == true
    val fresh = f.location?.let { it.kind != LocationKind.HIDDEN && now - it.timestamp < 10 * 60 } == true
    val status = when {
        !f.active -> "ждём ответа"
        else -> locationStatus(f, now) ?: "пока нет позиции"
    }
    ListRow(
        title = f.nick ?: "Без имени",
        subtitle = if (f.active && !f.verified) "$status · не проверен — сверьте код безопасности" else status,
        subtitleColor = if (f.active && !f.verified) Ink.danger else if (fresh) Ink.fresh else Ink.mist,
        leading = { AvatarCircle(f) },
        trailing = {
            Text(
                "Ещё",
                style = MaterialTheme.typography.labelLarge,
                color = Ink.mint,
                modifier = Modifier.clip(RoundedCornerShape(12.dp)).clickable(onClick = onMore).padding(horizontal = 10.dp, vertical = 8.dp),
            )
        },
        enabled = visible,
        onClick = if (visible) onShow else null,
    )
}

/** Настройки: передача, разрешения, профиль, диагностика. */
@Composable
private fun SettingsScreen(core: StayaCore, nick: String, server: String) {
    DarkScreen("настройки", onBack = ::home) {
        Message()
        SharingCard(core)
        SectionTitle("аккаунт")
        ListRow("Ник", nick)
        ListRow("Сервер", server)
        Text(
            "Координаты шифруются на телефоне — сервер видит только зашифрованные пакеты.",
            style = MaterialTheme.typography.bodySmall,
            color = Ink.mist,
        )
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
    DarkScreen("мой QR-код", onBack = ::home) {
        Text("Покажи код другу рядом. Добавленный по QR при встрече друг сразу считается проверенным.", color = Ink.mist)
        Box(
            modifier = Modifier.fillMaxWidth().clip(RoundedCornerShape(28.dp)).background(Ink.paper).padding(20.dp),
            contentAlignment = Alignment.Center,
        ) {
            if (left > 0) {
                Image(
                    bitmap,
                    contentDescription = "QR-код приглашения",
                    filterQuality = FilterQuality.None,
                    modifier = Modifier.fillMaxWidth().aspectRatio(1f),
                )
            } else {
                Text("Код истёк", style = MaterialTheme.typography.titleLarge, color = Ink.ink, modifier = Modifier.padding(vertical = 60.dp))
            }
        }
        if (left > 0) {
            Text(
                "Действует ещё ${left / 60_000}:${"%02d".format((left / 1000) % 60)}. Не закрывай приложение, пока друг не отсканирует.",
                color = Ink.mist,
            )
        }
        Pill("Новый код", { FriendsModel.invite(InviteMethod.QR) })
    }
}

@Composable
private fun ShareLink(uri: String) {
    val context = LocalContext.current
    DarkScreen("ссылка-приглашение", onBack = ::home) {
        Text(
            "Ссылка действует 24 часа, сработает один раз и откроется прямо из мессенджера. В ней адрес сервера " +
                "и код регистрации — другу ничего вводить не нужно.",
            color = Ink.paper,
        )
        Text(
            "Если друг ответит, а у тебя выключено «Делиться позицией», открой Staya в течение суток. " +
                "Добавленный по ссылке друг будет «не проверен», пока вы не сверите код безопасности.",
            style = MaterialTheme.typography.bodySmall,
            color = Ink.mist,
        )
        Pill("Отправить", {
            // https-вид (protocol §5.4): staya:// мессенджеры не делают нажимаемым.
            val text = uniffi.staya_core.webLink(uri) ?: uri
            val send = Intent(Intent.ACTION_SEND).setType("text/plain").putExtra(Intent.EXTRA_TEXT, text)
            context.startActivity(Intent.createChooser(send, "Отправить приглашение"))
        })
    }
}

@Composable
private fun Scan() {
    val context = LocalContext.current
    var granted by remember {
        mutableStateOf(context.checkSelfPermission(android.Manifest.permission.CAMERA) == android.content.pm.PackageManager.PERMISSION_GRANTED)
    }
    val ask = androidx.activity.compose.rememberLauncherForActivityResult(
        androidx.activity.result.contract.ActivityResultContracts.RequestPermission(),
    ) { granted = it }
    DarkScreen("QR-код друга", onBack = ::home) {
        if (granted) {
            Box(modifier = Modifier.fillMaxWidth().aspectRatio(1f).clip(RoundedCornerShape(28.dp))) {
                QrScanner(Modifier.fillMaxSize()) { FriendsModel.open(it, scanned = true) }
            }
            Text("Наведи камеру на QR-код в Staya друга. Кадры никуда не сохраняются и не отправляются.", color = Ink.mist)
        } else {
            Text("Чтобы отсканировать QR-код, нужна камера. Разрешение — только по нажатию.", color = Ink.mist)
            Pill("Разрешить камеру", { ask.launch(android.Manifest.permission.CAMERA) })
        }
    }
}

@Composable
private fun Confirm(s: FriendsModel.Screen.Confirm) {
    DarkScreen("добавить друга?", onBack = ::home) {
        Text("Этот человек будет видеть, где ты, пока ты не скроешь позицию или не удалишь его.", color = Ink.paper)
        ListRow("Сервер", s.info.server)
        Text(
            // Способ — как приглашение получено здесь, а не что написано в ссылке (§5.1).
            if (s.scanned && s.info.method == InviteMethod.QR) {
                "Код отсканирован при встрече — друг будет проверенным."
            } else {
                "Приглашение получено ссылкой — сверьте потом код безопасности."
            },
            style = MaterialTheme.typography.bodySmall,
            color = Ink.mist,
        )
        Message()
        Pill("Добавить", { FriendsModel.accept(s.uri, s.scanned) }, enabled = !FriendsModel.busy)
    }
}

@Composable
private fun ConfirmServerCode(s: FriendsModel.Screen.ConfirmServerCode) {
    DarkScreen("обновить код?", onBack = ::home) {
        Text(
            "В ссылке новый код регистрации для сервера ${s.host}. Он уйдёт в твои приглашения, " +
                "чтобы друзьям не нужно было его вводить.",
            color = Ink.paper,
        )
        Text(
            "Обновляй, только если ссылку прислал владелец сервера: с чужим кодом друзья не смогут зарегистрироваться.",
            style = MaterialTheme.typography.bodySmall,
            color = Ink.mist,
        )
        Pill("Обновить", { FriendsModel.updateCode(s.uri) })
        SoftButton(onClick = ::home) { Text("Отмена") }
    }
}

@Composable
private fun FriendCard(initial: FriendView, code: String) {
    // Свежая версия из списка: точность и статус меняются, пока карточка открыта.
    val friend = FriendsModel.friends.firstOrNull { it.accountId == initial.accountId } ?: initial
    var confirmRemove by remember { mutableStateOf(false) }
    DarkScreen(friend.nick ?: "друг", onBack = ::home) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            AvatarCircle(friend, size = 64)
            Spacer(Modifier.width(16.dp))
            Column {
                Text(
                    when {
                        !friend.active -> "ждём ответа"
                        friend.verified -> "проверен"
                        else -> "не проверен"
                    },
                    style = MaterialTheme.typography.titleMedium,
                    color = if (friend.active && !friend.verified) Ink.danger else Ink.paper,
                )
                locationStatus(friend, System.currentTimeMillis() / 1000)?.let {
                    Text(it, style = MaterialTheme.typography.bodySmall, color = Ink.mist)
                }
            }
        }
        Message()

        if (friend.active) {
            SectionTitle("что видит этот друг")
            listOf(
                Precision.EXACT to "Точную позицию",
                Precision.APPROX to "Примерно (район ~1 км)",
                Precision.HIDDEN to "Ничего — «скрыл(а) позицию»",
            ).forEach { (p, title) ->
                Row(
                    modifier = Modifier.fillMaxWidth().clip(RoundedCornerShape(16.dp)).clickable { FriendsModel.setPrecision(friend, p) }.padding(vertical = 4.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    RadioButton(
                        selected = friend.precision == p,
                        onClick = { FriendsModel.setPrecision(friend, p) },
                        colors = RadioButtonDefaults.colors(selectedColor = Ink.mint, unselectedColor = Ink.mist),
                    )
                    Text(title, color = Ink.paper)
                }
            }
        }

        SectionTitle("код безопасности")
        Text(
            code.chunked(5).joinToString(" "),
            style = MaterialTheme.typography.titleMedium.copy(fontFamily = FontFamily.Monospace, fontWeight = FontWeight.Medium),
            color = Ink.paper,
        )
        Text(
            "Сравните код при встрече или голосом. Совпадает — значит, между вами никого нет. " +
                "Если код другой — не нажимай «Совпадает» и удали этого друга.",
            style = MaterialTheme.typography.bodySmall,
            color = Ink.mist,
        )
        if (friend.verified) {
            Text("Уже проверен.", color = Ink.fresh)
        } else if (friend.active) {
            Pill("Совпадает", { FriendsModel.markVerified(friend) })
        }

        Spacer(Modifier.height(8.dp))
        SoftButton(onClick = { confirmRemove = true }) { Text("Удалить друга", color = Ink.danger) }
    }
    if (confirmRemove) {
        AlertDialog(
            onDismissRequest = { confirmRemove = false },
            containerColor = Ink.paper,
            titleContentColor = Ink.ink,
            textContentColor = Ink.ink,
            title = { Text("Удалить ${friend.nick ?: "друга"}?") },
            text = { Text("Он сразу перестанет видеть твою позицию, а ты — его. Чтобы снова дружить, придётся добавить друг друга заново.") },
            confirmButton = {
                TextButton(onClick = {
                    confirmRemove = false
                    FriendsModel.remove(friend)
                }) { Text("Удалить", color = Ink.danger) }
            },
            dismissButton = { TextButton(onClick = { confirmRemove = false }) { Text("Отмена", color = Ink.ink) } },
        )
    }
}
