package io.github.realfamousbae.staya.net

import kotlin.random.Random

/**
 * Экспоненциальная задержка с разбросом для повторов: сеть, 5xx, переподключение
 * WebSocket. После 429 — не меньше [RATE_LIMITED_MS]: лимиты сервера (задача 3.6)
 * общие для всех за одним NAT, частые повторы заперли бы и соседей.
 */
class Backoff(
    private val baseMs: Long = 1_000,
    private val maxMs: Long = 5 * 60_000,
    private val random: Random = Random.Default,
) {
    private var attempt = 0

    fun next(rateLimited: Boolean = false): Long {
        val exp = (baseMs shl attempt.coerceAtMost(20)).coerceAtMost(maxMs)
        attempt++
        // Разброс 50–100 %: клиенты не повторяют одновременно.
        val delay = exp / 2 + random.nextLong(exp / 2 + 1)
        return if (rateLimited) maxOf(delay, RATE_LIMITED_MS) else delay
    }

    fun reset() {
        attempt = 0
    }

    companion object {
        const val RATE_LIMITED_MS = 30_000L
    }
}
