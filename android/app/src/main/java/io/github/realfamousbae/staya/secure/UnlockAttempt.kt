package io.github.realfamousbae.staya.secure

/** A biometric result is valid once, and only for the current foreground attempt. */
internal class UnlockAttempt {
    private var serial = 0L
    private var pending: Long? = null
    fun begin(): Long = (++serial).also { pending = it }
    fun invalidate() { pending = null }
    fun finish(token: Long): Boolean {
        if (pending != token) return false
        pending = null
        return true
    }
}
