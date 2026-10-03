package io.github.realfamousbae.staya.probe

import java.net.HttpURLConnection
import java.net.URL

/** Отправка метрики на сборщик. Без сторонних библиотек: встроенный HttpURLConnection. */
object ProbeClient {
    const val ENDPOINT = "https://2-27-42-60.sslip.io/probe"

    data class Result(val status: Int?, val millis: Long) {
        val ok: Boolean get() = status == 204
    }

    /** Вызывать не с главного потока. */
    fun send(json: String, token: String, endpoint: String = ENDPOINT): Result {
        val start = System.nanoTime()
        val status = runCatching {
            val conn = URL(endpoint).openConnection() as HttpURLConnection
            try {
                conn.requestMethod = "POST"
                conn.connectTimeout = 8_000
                conn.readTimeout = 8_000
                conn.doOutput = true
                conn.setRequestProperty("Content-Type", "application/json")
                conn.setRequestProperty("Authorization", "Bearer $token")
                conn.outputStream.use { it.write(json.toByteArray(Charsets.UTF_8)) }
                conn.responseCode
            } finally {
                conn.disconnect()
            }
        }.getOrNull()
        return Result(status, (System.nanoTime() - start) / 1_000_000)
    }
}
