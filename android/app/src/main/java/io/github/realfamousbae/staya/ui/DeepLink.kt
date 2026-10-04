package io.github.realfamousbae.staya.ui

/** Ссылка, открытая в приложении: из камеры, буфера или другого приложения. */
sealed interface DeepLink {
    val uri: String

    /** `staya://add?…` — приглашение друга. Только экран подтверждения. */
    data class Invite(override val uri: String) : DeepLink

    /** `staya://server?…` — ссылка на сервер для онбординга. */
    data class Server(override val uri: String) : DeepLink

    companion object {
        fun parse(text: String?): DeepLink? {
            val t = text?.trim() ?: return null
            return when {
                t.startsWith("staya://add?") -> Invite(t)
                t.startsWith("staya://server?") -> Server(t)
                else -> null
            }
        }
    }
}
