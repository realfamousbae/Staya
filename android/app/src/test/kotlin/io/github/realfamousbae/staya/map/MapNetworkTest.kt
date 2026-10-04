package io.github.realfamousbae.staya.map

import java.io.IOException
import java.nio.file.Files
import java.security.MessageDigest
import mockwebserver3.MockResponse
import mockwebserver3.MockWebServer
import okhttp3.OkHttpClient
import okhttp3.Request
import okhttp3.tls.HandshakeCertificates
import okhttp3.tls.HeldCertificate
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Assert.fail
import org.junit.Before
import org.junit.Test
import uniffi.staya_core.StayaCore

/**
 * Сеть карты (4.4) с настоящим TLS и настоящим ядром: тайлы и стиль идут только к
 * привязанному серверу и только после проверки его ключа (protocol §5.3).
 */
class MapNetworkTest {
    private val ca = HeldCertificate.Builder().certificateAuthority(0).commonName("Staya test CA").build()
    private val leaf = HeldCertificate.Builder().signedBy(ca).addSubjectAlternativeName("localhost").build()
    private val otherLeaf = HeldCertificate.Builder().signedBy(ca).addSubjectAlternativeName("localhost").build()
    private lateinit var server: MockWebServer
    private val cores = mutableListOf<StayaCore>()

    private fun pin(cert: HeldCertificate): ByteArray =
        MessageDigest.getInstance("SHA-256").digest(cert.certificate.publicKey.encoded)

    @Before
    fun setUp() {
        server = MockWebServer()
        server.useHttps(HandshakeCertificates.Builder().heldCertificate(leaf).build().sslSocketFactory())
        repeat(3) { server.enqueue(MockResponse.Builder().body("{}").build()) }
        server.start(java.net.InetAddress.getByName("127.0.0.1"), 0)
    }

    @After
    fun tearDown() {
        server.close()
        cores.forEach { it.close() }
    }

    private val port get() = server.url("/").port

    private fun client(pins: List<ByteArray>): OkHttpClient {
        val path = Files.createTempDirectory("staya-map").resolve("staya.db").toString()
        val core = StayaCore.open(path, ByteArray(32) { 2 }).also {
            it.setServer("localhost:$port", pins)
            cores += it
        }
        val trust = HandshakeCertificates.Builder().addTrustedCertificate(ca.certificate).build()
        return MapNetwork.buildClient(
            core,
            OkHttpClient.Builder()
                .sslSocketFactory(trust.sslSocketFactory(), trust.trustManager)
                .dns { listOf(java.net.InetAddress.getByName("127.0.0.1")) },
        )
    }

    private fun get(c: OkHttpClient, url: String) = c.newCall(Request.Builder().url(url).build()).execute().use { it.code }

    @Test
    fun pinnedServerServesTiles() {
        assertEquals(200, get(client(listOf(pin(leaf))), "https://localhost:$port/tiles/region/1/2/3.mvt"))
        assertEquals(1, server.requestCount)
    }

    @Test
    fun wrongKeySendsNoMapRequest() {
        val c = client(listOf(pin(otherLeaf)))
        try {
            get(c, "https://localhost:$port/tiles/region/1/2/3.mvt")
            fail("map request reached a server with an unpinned key")
        } catch (_: IOException) {
        }
        assertEquals("no tile request may reach the server", 0, server.requestCount)
    }

    @Test
    fun otherHostOrPortIsBlockedBeforeConnecting() {
        val c = client(listOf(pin(leaf)))
        for (url in listOf(
            "https://127.0.0.1:$port/map/style-light.json", // другое имя
            "https://localhost:${port + 1}/map/style-light.json", // другой порт
            "http://localhost:$port/map/style-light.json", // без TLS
        )) {
            try {
                get(c, url)
                fail("request to $url must be blocked")
            } catch (e: IOException) {
                assertTrue("$url: $e", e.message?.contains("outside the bound server") == true)
            }
        }
        assertEquals(0, server.requestCount)
    }
}
