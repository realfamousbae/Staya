package io.github.realfamousbae.staya.net

import java.util.concurrent.Executors
import java.util.concurrent.ScheduledExecutorService
import java.util.concurrent.ScheduledFuture
import java.util.concurrent.TimeUnit
import okhttp3.Response
import okhttp3.WebSocket
import okhttp3.WebSocketListener
import uniffi.staya_core.CoreEvent

/**
 * Живая доставка, пока приложение на экране (protocol §8.2.6): WebSocket, после
 * подключения — один полный забор ящика, затем события по одному. Обрыв (в том
 * числе плановое закрытие сервером через час) — переподключение с [Backoff].
 * В фоне не используется: фоновые пробуждения (4.5) делают один `CoreSync.sync()`.
 *
 * Все обращения к ядру и сети идут в одном потоке — по порядку.
 */
class LiveConnection(
    private val client: StayaClient,
    private val sync: CoreSync,
    private val onEvents: (List<CoreEvent>) -> Unit,
    private val onError: (Exception) -> Unit = {},
    /** Общий поток с остальной сетью приложения: отправки ядра не должны идти параллельно. */
    private val worker: ScheduledExecutorService = Executors.newSingleThreadScheduledExecutor(),
) {
    private val backoff = Backoff()
    private var socket: WebSocket? = null
    private var reconnect: ScheduledFuture<*>? = null
    @Volatile private var running = false

    fun start() {
        running = true
        worker.execute { connect() }
    }

    fun stop() {
        running = false
        worker.execute {
            reconnect?.cancel(false)
            socket?.close(1000, null)
            socket = null
        }
    }

    private fun connect() {
        if (!running || socket != null) return
        try {
            socket = client.openWebSocket(listener)
        } catch (e: Exception) {
            onError(e)
            retry(rateLimited = e is RateLimitedException)
        }
    }

    private fun retry(rateLimited: Boolean) {
        socket = null
        // Ключ сервера отвергнут — повторы не помогут, решает пользователь (4.2).
        if (!running) return
        reconnect = worker.schedule({ connect() }, backoff.next(rateLimited), TimeUnit.MILLISECONDS)
    }

    private val listener = object : WebSocketListener() {
        override fun onOpen(webSocket: WebSocket, response: Response) {
            worker.execute {
                backoff.reset()
                // Всё, что пришло до подключения, — из ящика.
                runCatching { onEvents(sync.sync()) }.onFailure { onError(it as? Exception ?: Exception(it)) }
            }
        }

        override fun onMessage(webSocket: WebSocket, text: String) {
            worker.execute {
                runCatching { onEvents(sync.handleWsEvent(text)) }
                    .onFailure { onError(it as? Exception ?: Exception(it)) }
            }
        }

        override fun onClosing(webSocket: WebSocket, code: Int, reason: String) {
            webSocket.close(1000, null)
        }

        override fun onClosed(webSocket: WebSocket, code: Int, reason: String) {
            worker.execute { if (socket === webSocket) retry(rateLimited = false) }
        }

        override fun onFailure(webSocket: WebSocket, t: Throwable, response: Response?) {
            worker.execute {
                if (socket !== webSocket) return@execute
                onError(t as? Exception ?: Exception(t))
                if (t is javax.net.ssl.SSLPeerUnverifiedException) {
                    // Ключ или имя сервера не прошли проверку — не переподключаемся.
                    running = false
                    socket = null
                    return@execute
                }
                retry(rateLimited = response?.code == 429)
            }
        }
    }
}
