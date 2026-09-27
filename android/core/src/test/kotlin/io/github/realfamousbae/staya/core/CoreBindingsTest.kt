package io.github.realfamousbae.staya.core

import org.junit.Assert.assertEquals
import org.junit.Test
import uniffi.staya_core.coreVersion

/** Проверяет, что Kotlin-привязки реально вызывают Rust-ядро. */
class CoreBindingsTest {
    @Test
    fun coreVersionComesFromRust() {
        assertEquals("0.1.0", coreVersion())
    }
}
