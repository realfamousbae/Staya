package io.github.realfamousbae.staya.map

import kotlin.math.PI
import kotlin.math.asin
import kotlin.math.atan2
import kotlin.math.cos
import kotlin.math.sin

/** Вспомогательное для карты без зависимостей от MapLibre (проверяется JVM-тестами). */
object MapMath {
    private const val EARTH_RADIUS_M = 6_371_008.8

    /**
     * Круг точности как многоугольник в градусах: [lon, lat], замкнутый (последняя
     * точка = первая). Радиус в метрах честный на любом масштабе карты.
     */
    fun accuracyRing(latE7: Int, lonE7: Int, radiusM: Double, points: Int = 48): List<DoubleArray> {
        val lat = Math.toRadians(latE7 / 1e7)
        val lon = Math.toRadians(lonE7 / 1e7)
        val d = radiusM / EARTH_RADIUS_M
        val ring = (0 until points).map { i ->
            val bearing = 2 * PI * i / points
            val lat2 = asin(sin(lat) * cos(d) + cos(lat) * sin(d) * cos(bearing))
            val lon2 = lon + atan2(sin(bearing) * sin(d) * cos(lat), cos(d) - sin(lat) * sin(lat2))
            doubleArrayOf(Math.toDegrees(lon2), Math.toDegrees(lat2))
        }
        return ring + listOf(ring.first())
    }

    /** «только что», «5 мин назад», «3 ч назад», «2 дн назад». */
    fun ageLabel(nowSec: Long, timestamp: Long): String {
        val age = (nowSec - timestamp).coerceAtLeast(0)
        return when {
            age < 60 -> "только что"
            age < 3600 -> "${age / 60} мин назад"
            age < 86_400 -> "${age / 3600} ч назад"
            else -> "${age / 86_400} дн назад"
        }
    }

    /**
     * `bounds` из TileJSON сервера карты (`/tiles/region.json`): запад, юг, восток,
     * север. Без org.json — в JVM-тестах Android он заглушка.
     */
    fun tileJsonBounds(json: String): DoubleArray? {
        val match = Regex("\"bounds\"\\s*:\\s*\\[([^\\]]*)]").find(json) ?: return null
        val values = match.groupValues[1].split(',').map { it.trim().toDoubleOrNull() ?: return null }
        if (values.size != 4) return null
        val (west, south, east, north) = values
        if (west !in -180.0..180.0 || east !in -180.0..180.0 || south !in -90.0..90.0 || north !in -90.0..90.0) {
            return null
        }
        if (west >= east || south >= north) return null
        return doubleArrayOf(west, south, east, north)
    }
}
