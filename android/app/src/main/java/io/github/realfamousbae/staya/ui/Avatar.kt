package io.github.realfamousbae.staya.ui

import android.content.Context
import android.graphics.Bitmap
import android.graphics.ImageDecoder
import android.net.Uri
import java.io.ByteArrayOutputStream
import kotlin.math.max
import kotlin.math.roundToInt
import androidx.core.graphics.scale

/**
 * Аватар для профиля: квадрат 128×128 в JPEG не больше 8 КБ (protocol §6,
 * `AVATAR_MAX_LEN`) — профиль уходит друзьям в одном управляющем сообщении.
 */
object Avatar {
    const val MAX_BYTES = 8192
    private const val SIDE = 128

    /** `null` — не удалось открыть картинку или уменьшить до лимита. */
    fun encode(context: Context, uri: Uri): ByteArray? = runCatching {
        val source = ImageDecoder.createSource(context.contentResolver, uri)
        val bitmap = ImageDecoder.decodeBitmap(source) { decoder, info, _ ->
            // Сразу уменьшаем при декодировании: фото с камеры огромные.
            val s = info.size
            val k = max(SIDE.toFloat() * 2 / minOf(s.width, s.height), 1f / 16)
            if (k < 1f) decoder.setTargetSize((s.width * k).roundToInt(), (s.height * k).roundToInt())
            decoder.allocator = ImageDecoder.ALLOCATOR_SOFTWARE
        }
        val square = cropSquare(bitmap).scale(SIDE, SIDE)
        (80 downTo 10 step 10).firstNotNullOfOrNull { quality ->
            ByteArrayOutputStream().use { out ->
                square.compress(Bitmap.CompressFormat.JPEG, quality, out)
                out.toByteArray().takeIf { it.size <= MAX_BYTES }
            }
        }
    }.getOrNull()

    private fun cropSquare(b: Bitmap): Bitmap {
        val side = minOf(b.width, b.height)
        return Bitmap.createBitmap(b, (b.width - side) / 2, (b.height - side) / 2, side, side)
    }
}
