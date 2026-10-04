package io.github.realfamousbae.staya.ui

import com.google.zxing.RGBLuminanceSource
import com.google.zxing.qrcode.decoder.ErrorCorrectionLevel
import com.google.zxing.qrcode.encoder.Encoder
import java.nio.file.Files
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import uniffi.staya_core.InviteMethod
import uniffi.staya_core.StayaCore

/** QR приглашения тем же кодировщиком, что в приложении (задача 4.3). */
class QrTest {
    private fun invite(host: String, pins: List<ByteArray>): String {
        val path = Files.createTempDirectory("staya").resolve("staya.db").toString()
        return StayaCore.open(path, ByteArray(32) { 3 }).use { core ->
            core.setServer(host, pins)
            core.createInvite(InviteMethod.QR, 1_700_000_000)
        }
    }

    @Test
    fun realInviteRoundTripsThroughQr() {
        // Худший случай: длинное имя sslip.io с портом и два отпечатка.
        val uri = invite("255-255-255-255.sslip.io:8443", listOf(ByteArray(32) { 1 }, ByteArray(32) { 2 }))
        val qr = Encoder.encode(uri, ErrorCorrectionLevel.M)
        assertTrue("QR version ${qr.version.versionNumber} for ${uri.length} bytes", qr.version.versionNumber <= 13)

        // Как на экране: модуль — несколько пикселей (детектору нужен масштаб кадра камеры).
        val m = Qr.matrix(uri)
        val k = 6
        val w = m.width * k
        val h = m.height * k
        val pixels = IntArray(w * h) { i ->
            if (m[(i % w) / k, (i / w) / k]) 0xFF000000.toInt() else 0xFFFFFFFF.toInt()
        }
        assertEquals(uri, Qr.decode(RGBLuminanceSource(w, h, pixels)))
        assertEquals(DeepLink.Invite(uri), DeepLink.parse(uri))
    }

    @Test
    fun deepLinksAreClassified() {
        assertTrue(DeepLink.parse(" staya://server?v=1&s=a.example ") is DeepLink.Server)
        assertNull(DeepLink.parse("https://example.com"))
        assertNull(DeepLink.parse(null))
        assertNull(DeepLink.parse("staya://other?x"))
    }
}
