package io.github.realfamousbae.staya.ui

import android.graphics.Bitmap
import androidx.core.graphics.createBitmap
import androidx.core.graphics.set
import com.google.zxing.BarcodeFormat
import com.google.zxing.BinaryBitmap
import com.google.zxing.DecodeHintType
import com.google.zxing.EncodeHintType
import com.google.zxing.LuminanceSource
import com.google.zxing.common.BitMatrix
import com.google.zxing.common.HybridBinarizer
import com.google.zxing.qrcode.QRCodeReader
import com.google.zxing.qrcode.QRCodeWriter
import com.google.zxing.qrcode.decoder.ErrorCorrectionLevel

/**
 * QR-коды приглашений. Коррекция M: на ней основана граница длины приглашения
 * (proto: версия 13 вмещает 331 байт и уверенно сканируется с экрана).
 */
object Qr {
    fun matrix(text: String): BitMatrix = QRCodeWriter().encode(
        text,
        BarcodeFormat.QR_CODE,
        0,
        0,
        mapOf(EncodeHintType.ERROR_CORRECTION to ErrorCorrectionLevel.M, EncodeHintType.MARGIN to 2),
    )

    /** Чёрно-белая картинка по одному пикселю на модуль — растягивается без сглаживания. */
    fun bitmap(text: String): Bitmap {
        val m = matrix(text)
        val bmp = createBitmap(m.width, m.height)
        for (y in 0 until m.height) {
            for (x in 0 until m.width) {
                bmp[x, y] = if (m[x, y]) 0xFF000000.toInt() else 0xFFFFFFFF.toInt()
            }
        }
        return bmp
    }

    /** Текст QR-кода из кадра или `null`. */
    fun decode(source: LuminanceSource): String? = runCatching {
        QRCodeReader().decode(
            BinaryBitmap(HybridBinarizer(source)),
            mapOf(DecodeHintType.POSSIBLE_FORMATS to listOf(BarcodeFormat.QR_CODE), DecodeHintType.TRY_HARDER to true),
        ).text
    }.getOrNull()
}
