package io.github.realfamousbae.staya.ui

import uniffi.staya_core.normalizeLink

/**
 * Ссылка, открытая в приложении: из камеры, буфера или другого приложения.
 * `uri` — всегда `staya://…`: https-вид из мессенджера (protocol §5.4) приводит ядро.
 */
sealed interface DeepLink {
    val uri: String

    /** `staya://add?…` — приглашение друга. Только экран подтверждения. */
    data class Invite(override val uri: String) : DeepLink

    /** `staya://server?…` — ссылка на сервер для онбординга. */
    data class Server(override val uri: String) : DeepLink

    companion object {
        fun parse(text: String?): DeepLink? {
            val t = normalizeLink(text ?: return null) ?: return null
            return when {
                t.startsWith("staya://add?") -> Invite(t)
                t.startsWith("staya://server?") -> Server(t)
                else -> null
            }
        }
    }
}
