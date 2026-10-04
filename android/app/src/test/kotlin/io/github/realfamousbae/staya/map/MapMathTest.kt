package io.github.realfamousbae.staya.map

import kotlin.math.PI
import kotlin.math.asin
import kotlin.math.cos
import kotlin.math.sin
import kotlin.math.sqrt
import okhttp3.HttpUrl.Companion.toHttpUrl
import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class MapMathTest {
    private fun distanceM(lat1: Double, lon1: Double, lat2: Double, lon2: Double): Double {
        val r = 6_371_008.8
        val p1 = lat1 * PI / 180
        val p2 = lat2 * PI / 180
        val dp = p2 - p1
        val dl = (lon2 - lon1) * PI / 180
        val a = sin(dp / 2) * sin(dp / 2) + cos(p1) * cos(p2) * sin(dl / 2) * sin(dl / 2)
        return 2 * r * asin(sqrt(a))
    }

    @Test
    fun accuracyRingIsClosedAndAtTheRadius() {
        val lat = 557_558_000
        val lon = 376_173_000
        val ring = MapMath.accuracyRing(lat, lon, 250.0)
        assertEquals(49, ring.size)
        assertArrayEquals(ring.first(), ring.last(), 0.0)
        for (p in ring) {
            val d = distanceM(lat / 1e7, lon / 1e7, p[1], p[0])
            assertEquals(250.0, d, 0.5)
        }
    }

    @Test
    fun ageLabels() {
        val now = 1_700_000_000L
        assertEquals("только что", MapMath.ageLabel(now, now - 5))
        assertEquals("только что", MapMath.ageLabel(now, now + 120)) // часы друга спешат
        assertEquals("5 мин назад", MapMath.ageLabel(now, now - 5 * 60 - 30))
        assertEquals("3 ч назад", MapMath.ageLabel(now, now - 3 * 3600 - 59))
        assertEquals("2 дн назад", MapMath.ageLabel(now, now - 2 * 86_400 - 1))
    }

    @Test
    fun tileJsonBoundsFromTheServer() {
        val json = """{"attribution":"x","bounds": [
            35.1, 54.2,
            40.25, 57 ],"center":[37.675,55.6,0]}"""
        assertArrayEquals(doubleArrayOf(35.1, 54.2, 40.25, 57.0), MapMath.tileJsonBounds(json), 0.0)
        assertNull(MapMath.tileJsonBounds("""{"center":[1,2,3]}"""))
        assertNull(MapMath.tileJsonBounds("""{"bounds":[1,2,3]}"""))
        assertNull(MapMath.tileJsonBounds("""{"bounds":[40,54,35,57]}"""))
        assertNull(MapMath.tileJsonBounds("""{"bounds":[1,2,"x",4]}"""))
    }

    @Test
    fun mapRequestsOnlyToTheBoundServerOverHttps() {
        val host = "staya.example"
        assertTrue(MapNetwork.allowed("https://staya.example/tiles/region/1/2/3.mvt".toHttpUrl(), host))
        assertFalse(MapNetwork.allowed("http://staya.example/map/style-light.json".toHttpUrl(), host))
        assertFalse(MapNetwork.allowed("https://demotiles.maplibre.org/style.json".toHttpUrl(), host))
        assertFalse(MapNetwork.allowed("https://staya.example:8443/x".toHttpUrl(), host))
        assertFalse(MapNetwork.allowed("https://evil.staya.example/x".toHttpUrl(), host))
        assertTrue(MapNetwork.allowed("https://staya.example:8443/x".toHttpUrl(), "staya.example:8443"))
        assertFalse(MapNetwork.allowed("https://staya.example/x".toHttpUrl(), null))
    }
}
