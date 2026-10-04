package io.github.realfamousbae.staya.net

import java.io.IOException
import java.net.HttpURLConnection
import java.net.URL

/** Ответ сервера с кодом не 2xx. */
class HttpStatusException(val status: Int, path: String) : IOException("$path: HTTP $status")

/**
 * HTTP к серверу Staya (protocol §8.3): JSON и `Authorization: Bearer`. Блокирующий —
 * вызывать не на главном потоке. Пока без pinning: задача 2.10 ходит только на
 * dev-сервер по loopback, TLS с pinning и OkHttp — задача 4.1.
 */
class StayaHttp(baseUrl: String, private val token: () -> String) {
    private val base = baseUrl.trimEnd('/')

    fun get(path: String): String = call("GET", path, null)

    fun post(path: String, json: String): String = call("POST", path, json)

    fun put(path: String, json: String): String = call("PUT", path, json)

    fun delete(path: String): String = call("DELETE", path, null)

    /** Без авторизации и с телом-строкой: место встречи `/dev/invite` dev-сервера. */
    fun getPublic(path: String): String = call("GET", path, null, auth = false)

    private fun call(method: String, path: String, body: String?, auth: Boolean = true): String {
        val conn = URL(base + path).openConnection() as HttpURLConnection
        try {
            conn.requestMethod = method
            conn.connectTimeout = TIMEOUT_MS
            conn.readTimeout = TIMEOUT_MS
            conn.useCaches = false
            if (auth) conn.setRequestProperty("Authorization", "Bearer ${token()}")
            if (body != null) {
                conn.doOutput = true
                conn.setRequestProperty("Content-Type", "application/json")
                conn.outputStream.use { it.write(body.toByteArray(Charsets.UTF_8)) }
            }
            val status = conn.responseCode
            if (status !in 200..299) throw HttpStatusException(status, path)
            return conn.inputStream.use { it.readBytes().toString(Charsets.UTF_8) }
        } finally {
            conn.disconnect()
        }
    }

    private companion object {
        const val TIMEOUT_MS = 15_000
    }
}
