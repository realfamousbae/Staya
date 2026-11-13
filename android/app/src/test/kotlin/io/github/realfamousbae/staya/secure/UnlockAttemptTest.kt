package io.github.realfamousbae.staya.secure

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class UnlockAttemptTest {
    @Test fun backgroundDuringPromptCannotUnlockOnLateSuccess() {
        val attempt = UnlockAttempt()
        val old = attempt.begin()
        attempt.invalidate()
        assertFalse(attempt.finish(old))
    }
    @Test fun oldCallbackCannotCancelNewForegroundPrompt() {
        val attempt = UnlockAttempt()
        val old = attempt.begin()
        attempt.invalidate()
        val current = attempt.begin()
        assertFalse(attempt.finish(old))
        assertTrue(attempt.finish(current))
        assertFalse(attempt.finish(current))
    }
}
