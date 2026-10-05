package io.github.realfamousbae.staya.ui

import io.github.realfamousbae.staya.net.HttpStatusException
import io.github.realfamousbae.staya.net.InviteCodeRequiredException
import io.github.realfamousbae.staya.net.RateLimitedException
import io.github.realfamousbae.staya.net.ServerKeyRejectedException
import java.io.IOException
import java.net.SocketException
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/** Какие ошибки сети не показываются пользователю: соединение повторяется само. */
class TransientErrorTest {
    @Test
    fun dropsAndServerErrorsAreTransient() {
        assertTrue(AppModel.isTransient(SocketException("Software caused connection abort")))
        assertTrue(AppModel.isTransient(IOException("canceled")))
        assertTrue(AppModel.isTransient(RateLimitedException()))
        assertTrue(AppModel.isTransient(HttpStatusException(502, "/v1/mailbox")))
    }

    @Test
    fun securityAndActionableErrorsAreShown() {
        assertFalse(AppModel.isTransient(ServerKeyRejectedException()))
        assertFalse(AppModel.isTransient(InviteCodeRequiredException()))
        assertFalse(AppModel.isTransient(HttpStatusException(403, "/v1/accounts")))
        assertFalse(AppModel.isTransient(IllegalStateException("x")))
    }
}
