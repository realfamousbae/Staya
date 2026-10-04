package io.github.realfamousbae.staya.net

import java.io.IOException
import java.util.concurrent.TimeUnit
import okhttp3.MediaType.Companion.toMediaType
import okhttp3.OkHttpClient
import okhttp3.Request
import okhttp3.RequestBody.Companion.toRequestBody
import okhttp3.Response
import okhttp3.WebSocket
import okhttp3.WebSocketListener
import uniffi.staya_core.StayaCore

/** Ключ сервера не совпал с отпечатками или с запомненным: подключение запрещено (§5.3). */
class ServerKeyRejectedException : IOException("server key rejected")

/** Закрытый сервер: для регистрации нужен код приглашения. */
class InviteCodeRequiredException : IOException("invite code required")

/** Сервер ограничил частоту запросов (429). */
class RateLimitedException : IOException("rate limited")

/** Ответ не 2xx, кроме разобранных выше. */
class HttpStatusException(val status: Int, path: String) : IOException("$path: HTTP $status")

/**
 * HTTP к серверу Staya (protocol §4, §8.3) поверх OkHttp. Блокирующий: вызывать
 * не на главном потоке. Адрес — из привязки аккаунта к серверу в ядре; `baseUrl`
 * задаётся явно только для отладки (эмулятор → `http://10.0.2.2:…`).
 *
 * Вход: токен из ядра; нет токена или 401 — challenge и подпись ядром; 404 на
 * challenge — аккаунта на сервере нет (сервер потерял базу), регистрация теми же
 * ключами. Повтор после 401 — один раз; 429 не повторяется (см. [Backoff]).
 */
class StayaClient(
    private val core: StayaCore,
    private val baseUrl: String = "https://" + (core.server()?.host ?: throw IllegalStateException("no server")),
    /** Код приглашения на регистрацию закрытого сервера (онбординг, 4.2). */
    private val inviteCode: () -> String? = { null },
    builder: OkHttpClient.Builder = OkHttpClient.Builder(),
) {
    private val verifier: ServerKeyVerifier
    val http: OkHttpClient

    init {
        val base = builder
            .connectTimeout(15, TimeUnit.SECONDS)
            .readTimeout(30, TimeUnit.SECONDS)
            .writeTimeout(30, TimeUnit.SECONDS)
            // Пинг держит WebSocket через NAT и быстрее замечает обрыв.
            .pingInterval(30, TimeUnit.SECONDS)
            .build()
        verifier = ServerKeyVerifier(base.hostnameVerifier) { core.checkServerKey(it) }
        http = base.newBuilder().hostnameVerifier(verifier).build()
    }

    private fun now() = System.currentTimeMillis() / 1000

    fun get(path: String): String = authed { call("GET", path, null, it) }

    fun post(path: String, json: String): String = authed { call("POST", path, json, it) }

    fun put(path: String, json: String): String = authed { call("PUT", path, json, it) }

    fun delete(path: String): String = authed { call("DELETE", path, null, it) }

    /** WebSocket `/v1/ws` с текущим токеном (вход — до вызова, блокирующий). */
    fun openWebSocket(listener: WebSocketListener): WebSocket {
        val token = session()
        val url = baseUrl.trimEnd('/').replaceFirst("http", "ws") + "/v1/ws"
        val request = Request.Builder().url(url).header("Authorization", "Bearer $token").build()
        return http.newWebSocket(request, listener)
    }

    /** Действующий токен (base64); при необходимости — вход. */
    fun session(): String = core.sessionToken(now())?.let(::b64) ?: login()

    private fun <T> authed(block: (token: String) -> T): T {
        val token = session()
        return try {
            block(token)
        } catch (e: HttpStatusException) {
            if (e.status != 401) throw e
            // Токен отозван или сервер потерял базу: войти заново и повторить один раз.
            core.clearSession()
            block(login())
        }
    }

    private fun login(): String {
        val challenge = try {
            call("POST", "/v1/auth/challenge", core.authChallengeRequest(), null)
        } catch (e: HttpStatusException) {
            if (e.status != 404) throw e
            register()
            call("POST", "/v1/auth/challenge", core.authChallengeRequest(), null)
        }
        val verified = call("POST", "/v1/auth/verify", core.authVerifyRequest(challenge), null)
        core.completeLogin(verified)
        return core.sessionToken(now())?.let(::b64) ?: throw IOException("no session after login")
    }

    private fun register() {
        try {
            call("POST", "/v1/accounts", core.registerRequest(inviteCode()), null)
        } catch (e: HttpStatusException) {
            if (e.status == 403) throw InviteCodeRequiredException()
            throw e
        }
    }

    private fun call(method: String, path: String, json: String?, token: String?): String {
        val body = json?.toRequestBody(JSON)
        val builder = Request.Builder().url(baseUrl.trimEnd('/') + path).method(method, body)
        if (token != null) builder.header("Authorization", "Bearer $token")
        verifier.resetRejected()
        val response: Response = try {
            http.newCall(builder.build()).execute()
        } catch (e: IOException) {
            if (verifier.keyRejected()) throw ServerKeyRejectedException()
            throw e
        }
        response.use {
            when {
                it.isSuccessful -> return it.body.string()
                it.code == 429 -> throw RateLimitedException()
                else -> throw HttpStatusException(it.code, path)
            }
        }
    }

    private companion object {
        val JSON = "application/json".toMediaType()

        fun b64(bytes: ByteArray): String = java.util.Base64.getEncoder().encodeToString(bytes)
    }
}
