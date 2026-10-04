package io.github.realfamousbae.staya.ui

import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import io.github.realfamousbae.staya.net.CoreSync
import io.github.realfamousbae.staya.net.InviteCodeRequiredException
import io.github.realfamousbae.staya.net.RateLimitedException
import io.github.realfamousbae.staya.net.ServerKeyRejectedException
import io.github.realfamousbae.staya.net.StayaClient
import java.io.IOException
import java.util.concurrent.Executors
import uniffi.staya_core.CoreException
import uniffi.staya_core.StayaCore

/**
 * Что показывать: онбординг или главный экран (задача 4.2). Сеть и ядро — в
 * отдельном потоке, состояние — снимки Compose (их можно писать из любого потока).
 */
object AppModel {
    sealed interface Phase {
        data object Loading : Phase
        data object Onboarding : Phase
        data class Ready(val nick: String, val server: String, val accountId: String) : Phase
    }

    private val worker = Executors.newSingleThreadExecutor()

    var phase by mutableStateOf<Phase>(Phase.Loading)
        private set
    var busy by mutableStateOf(false)
        private set
    var error by mutableStateOf<String?>(null)

    /** Приложение на экране (onResume/onPause). */
    var foreground by mutableStateOf(false)

    /** Ссылка, открытая извне и ещё не показанная (онбординг или подтверждение). */
    var pendingLink by mutableStateOf<DeepLink?>(null)

    /** Есть привязка к серверу и ник — онбординг пройден. */
    fun refresh(core: StayaCore, accountId: String) {
        worker.execute {
            val server = runCatching { core.server() }.getOrNull()
            val nick = runCatching { core.myProfile().nick }.getOrDefault("")
            phase = if (server != null && nick.isNotEmpty()) Phase.Ready(nick, server.host, accountId) else Phase.Onboarding
        }
    }

    /** Привязка, профиль, регистрация и вход (внутри публикации ключей), затем приглашение друга. */
    fun createAccount(
        core: StayaCore,
        accountId: String,
        link: String,
        manualHost: String,
        inviteCode: String,
        nick: String,
        avatar: ByteArray?,
    ) {
        busy = true
        error = null
        worker.execute {
            val message = onboard(core, link, manualHost, inviteCode, nick, avatar)
            busy = false
            error = message
            if (message == null) refresh(core, accountId)
        }
    }

    /**
     * Онбординг без UI (тестируется на JVM): `null` — готово, иначе сообщение для
     * пользователя. `client` — для тестов (свой TLS); по умолчанию — сервер из привязки.
     */
    internal fun onboard(
        core: StayaCore,
        link: String,
        manualHost: String,
        inviteCode: String,
        nick: String,
        avatar: ByteArray?,
        client: (StayaCore, () -> String?) -> StayaClient = { c, code -> StayaClient(c, inviteCode = code) },
    ): String? {
        val l = link.trim()
        val host = manualHost.trim()
        val code = inviteCode.trim().ifEmpty { null }
        return try {
            when {
                l.isNotEmpty() -> core.setServerFromLink(l)
                host.isNotEmpty() -> core.setServer(host, emptyList())
                else -> throw NoServer()
            }
            core.setProfile(nick.trim(), avatar ?: ByteArray(0))
            val sync = CoreSync(core, client(core) { code })
            sync.publishKeys()
            if (l.startsWith("staya://add?")) sync.accept(l)
            null
        } catch (e: Exception) {
            // Пока друзей нет, неверный адрес можно исправить и попробовать снова.
            runCatching { core.resetServer() }
            describe(e)
        }
    }

    private class NoServer : Exception()

    fun describeError(e: Exception): String = describe(e)

    private fun describe(e: Exception): String = when (e) {
        is NoServer -> "Вставь приглашение друга или ссылку на сервер — или укажи сервер в «Дополнительно»."
        is InviteCodeRequiredException -> "Этот сервер закрытый: нужен код приглашения на регистрацию. Его даёт владелец сервера."
        is ServerKeyRejectedException -> "Ключ сервера не совпал с ожидаемым. Возможно, соединение перехватывают — не продолжай и спроси у того, кто дал ссылку."
        is RateLimitedException -> "Сервер просит подождать. Попробуй через минуту."
        is CoreException.ServerMismatch -> "Этот аккаунт уже привязан к другому серверу. Друзья должны быть на одном сервере."
        is CoreException.Proto, is CoreException.InvalidInvite -> "Не получилось разобрать ссылку. Проверь, что она скопирована целиком."
        is IOException -> "Нет связи с сервером. Проверь адрес и интернет."
        else -> "Не получилось: ${e.message ?: e.javaClass.simpleName}"
    }
}
