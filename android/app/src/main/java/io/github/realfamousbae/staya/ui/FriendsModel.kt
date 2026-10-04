package io.github.realfamousbae.staya.ui

import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import io.github.realfamousbae.staya.net.CoreSync
import io.github.realfamousbae.staya.net.LiveConnection
import io.github.realfamousbae.staya.net.StayaClient
import java.util.concurrent.Executors
import uniffi.staya_core.FriendView
import uniffi.staya_core.InviteInfo
import uniffi.staya_core.InviteMethod
import uniffi.staya_core.StayaCore

/**
 * Друзья (задача 4.3): приглашения, принятие после подтверждения, код
 * безопасности, живая синхронизация на экране. Вся сеть и работа с ядром — в
 * одном потоке [worker] (общий с [LiveConnection]): отправки ядра идут по очереди.
 */
object FriendsModel {
    sealed interface Screen {
        data object Home : Screen
        data class ShowQr(val uri: String, val expiresAt: Long) : Screen
        data class ShareLink(val uri: String) : Screen
        data object Scan : Screen
        data class Confirm(val uri: String, val info: InviteInfo) : Screen
        data class Safety(val friend: FriendView, val code: String) : Screen
    }

    private val worker = Executors.newSingleThreadScheduledExecutor()
    private var core: StayaCore? = null
    private var sync: CoreSync? = null
    private var live: LiveConnection? = null

    var screen by mutableStateOf<Screen>(Screen.Home)
    var friends by mutableStateOf<List<FriendView>>(emptyList())
        private set
    var busy by mutableStateOf(false)
        private set
    var message by mutableStateOf<String?>(null)

    /** Приложение на экране и аккаунт готов: ключи, ящик, WebSocket. */
    fun start(core: StayaCore) {
        worker.execute {
            if (this.core !== core) {
                live?.stop()
                this.core = core
                val client = runCatching { StayaClient(core) }.getOrElse {
                    message = AppModel.describeError(it as Exception)
                    return@execute
                }
                sync = CoreSync(core, client)
                live = LiveConnection(client, sync!!, { reload() }, { message = AppModel.describeError(it) }, worker)
            }
            reload()
            // Пополнить одноразовые ключи (их разбирают при добавлении) и повернуть fallback.
            runCatching { sync!!.publishKeys() }.onFailure { message = AppModel.describeError(it as Exception) }
            live!!.start()
        }
    }

    fun stop() {
        worker.execute { live?.stop() }
    }

    private fun reload() {
        friends = runCatching { core?.listFriends() }.getOrNull().orEmpty()
    }

    /** Новое приглашение: QR — 10 минут, ссылка — 24 часа (protocol §5). */
    fun invite(method: InviteMethod) {
        val core = core ?: return
        worker.execute {
            runCatching { core.createInvite(method, System.currentTimeMillis() / 1000) }
                .onSuccess { uri ->
                    screen = if (method == InviteMethod.QR) {
                        Screen.ShowQr(uri, System.currentTimeMillis() + QR_TTL_MS)
                    } else {
                        Screen.ShareLink(uri)
                    }
                }
                .onFailure { message = AppModel.describeError(it as Exception) }
        }
    }

    /** Ссылка из камеры, буфера или другого приложения: только экран подтверждения. */
    fun open(link: DeepLink) {
        val core = core ?: return
        if (link !is DeepLink.Invite) {
            message = "Это ссылка на сервер, а не приглашение. Сервер выбирается один раз — при создании аккаунта."
            return
        }
        worker.execute {
            runCatching { core.parseInvite(link.uri) }
                .onSuccess { info ->
                    val mine = runCatching { core.server()?.host }.getOrNull()
                    if (mine != null && mine != info.server) {
                        message = "Этот друг на другом сервере (${info.server}). Друзья должны быть на одном сервере."
                    } else {
                        screen = Screen.Confirm(link.uri, info)
                    }
                }
                .onFailure { message = "Не получилось разобрать приглашение. Проверь, что ссылка скопирована целиком." }
        }
    }

    /** Явное «Добавить» на экране подтверждения. */
    fun accept(uri: String) {
        busy = true
        worker.execute {
            val error = runCatching { sync!!.accept(uri) }.exceptionOrNull()
            busy = false
            if (error == null) {
                screen = Screen.Home
                message = "Запрос отправлен. Друг появится, когда его приложение будет на связи."
                reload()
            } else {
                message = AppModel.describeError(error as Exception)
            }
        }
    }

    fun showSafety(friend: FriendView) {
        val core = core ?: return
        worker.execute {
            runCatching { core.safetyCode(friend.accountId) }
                .onSuccess { screen = Screen.Safety(friend, it) }
                .onFailure { message = AppModel.describeError(it as Exception) }
        }
    }

    fun markVerified(friend: FriendView) {
        val core = core ?: return
        worker.execute {
            runCatching { core.markVerified(friend.accountId) }
                .onFailure { message = AppModel.describeError(it as Exception) }
            reload()
            screen = Screen.Home
        }
    }

    private const val QR_TTL_MS = 10 * 60_000L
}
