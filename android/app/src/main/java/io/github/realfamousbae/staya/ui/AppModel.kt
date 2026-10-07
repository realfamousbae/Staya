package io.github.realfamousbae.staya.ui

import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import io.github.realfamousbae.staya.net.CoreSync
import io.github.realfamousbae.staya.net.HttpStatusException
import io.github.realfamousbae.staya.net.InviteCodeRequiredException
import io.github.realfamousbae.staya.net.Net
import io.github.realfamousbae.staya.net.RateLimitedException
import io.github.realfamousbae.staya.net.ServerKeyRejectedException
import io.github.realfamousbae.staya.net.StayaClient
import java.io.IOException
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

    /** Общий поток сети ([Net.worker]): онбординг меняет привязку и исходящую очередь ядра. */
    private val worker = Net.worker

    var phase by mutableStateOf<Phase>(Phase.Loading)
        private set
    var busy by mutableStateOf(false)
        private set
    var error by mutableStateOf<String?>(null)

    /** Приложение на экране (onResume/onPause). */
    var foreground by mutableStateOf(false)

    /** Сервер ответил «нужен код регистрации»: онбординг сразу показывает поле. */
    var needCode by mutableStateOf(false)
        private set

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
        scanned: Boolean,
    ) {
        busy = true
        error = null
        worker.execute {
            val message = onboard(core, link, manualHost, inviteCode, nick, avatar, scanned)
            busy = false
            error = message
            if (message == null) refresh(core, accountId)
        }
    }

    /**
     * Онбординг без UI (тестируется на JVM): `null` — готово, иначе сообщение для
     * пользователя. `scanned` — приглашение отсканировано камерой приложения (§5.1).
     * Код регистрации — введённый вручную, иначе из ссылки (protocol §5.3); принятый
     * сервером код запоминается и уходит в приглашения. `client` — для тестов (свой
     * TLS); по умолчанию — сервер из привязки.
     */
    internal fun onboard(
        core: StayaCore,
        link: String,
        manualHost: String,
        inviteCode: String,
        nick: String,
        avatar: ByteArray?,
        scanned: Boolean = false,
        client: (StayaCore, () -> String?) -> StayaClient = { c, code -> StayaClient(c, inviteCode = code) },
    ): String? {
        val l = DeepLink.parse(link)?.uri ?: link.trim()
        val host = manualHost.trim()
        val code = inviteCode.trim().ifEmpty { null }
        return try {
            when {
                l.isNotEmpty() -> core.setServerFromLink(l)
                host.isNotEmpty() -> core.setServer(host, emptyList())
                else -> throw NoServer()
            }
            core.setProfile(nick.trim(), avatar ?: ByteArray(0))
            val sync = CoreSync(core, client(core) { code ?: core.server()?.registrationCode })
            sync.publishKeys()
            // Сервер принял код — друзьям по моим приглашениям вводить его не придётся.
            if (code != null) runCatching { core.setRegistrationCode(code) }
            needCode = false
            if (l.startsWith("staya://add?")) sync.accept(l, scanned)
            null
        } catch (e: Exception) {
            needCode = e is InviteCodeRequiredException
            // Пока друзей нет, неверный адрес можно исправить и попробовать снова.
            runCatching { core.resetServer() }
            describe(e)
        }
    }

    private class NoServer : Exception()

    fun describeError(e: Exception): String = describe(e)

    /**
     * Временный сбой сети (обрыв, нет связи, 429, 5xx): живое соединение и отправка
     * повторяются сами — пользователю его не показываем.
     */
    fun isTransient(e: Exception): Boolean = when (e) {
        is ServerKeyRejectedException, is InviteCodeRequiredException -> false
        is HttpStatusException -> e.status >= 500
        is IOException -> true
        else -> false
    }

    private fun describe(e: Exception): String = when (e) {
        is NoServer -> "Отсканируй QR друга, вставь его приглашение или ссылку на сервер — или укажи сервер в «Дополнительно»."
        is InviteCodeRequiredException -> "Этот сервер закрытый: нужен код регистрации. Впиши его ниже — его даёт владелец сервера."
        is ServerKeyRejectedException -> "Ключ сервера не совпал с ожидаемым. Возможно, соединение перехватывают — не продолжай и спроси у того, кто дал ссылку."
        is RateLimitedException -> "Сервер просит подождать. Попробуй через минуту."
        is CoreException.ServerMismatch -> "Этот аккаунт уже привязан к другому серверу. Друзья должны быть на одном сервере."
        is CoreException.Proto, is CoreException.InvalidInvite -> "Не получилось разобрать ссылку. Проверь, что она скопирована целиком."
        is IOException -> "Нет связи с сервером. Проверь адрес и интернет."
        else -> "Не получилось: ${e.message ?: e.javaClass.simpleName}"
    }
}
