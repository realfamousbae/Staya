package io.github.realfamousbae.staya.net

import java.nio.file.Files
import java.security.MessageDigest
import java.util.Base64
import java.util.concurrent.atomic.AtomicInteger
import javax.net.ssl.SSLException
import mockwebserver3.Dispatcher
import mockwebserver3.MockResponse
import mockwebserver3.MockWebServer
import mockwebserver3.RecordedRequest
import okhttp3.OkHttpClient
import okhttp3.tls.HandshakeCertificates
import okhttp3.tls.HeldCertificate
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Assert.fail
import org.junit.Before
import org.junit.Test
import uniffi.staya_core.StayaCore

/**
 * Сетевой слой с настоящим TLS (тестовый центр сертификации) и настоящим ядром:
 * правило доверия protocol §5.3 и вход §4. Главная проверка — при отвергнутом
 * ключе сервер не получает ни одного запроса, то есть токен не уходит.
 */
class StayaClientTest {
    private val ca = HeldCertificate.Builder().certificateAuthority(0).commonName("Staya test CA").build()
    private val leaf = leafFor("localhost")
    private val otherLeaf = leafFor("localhost")
    private lateinit var server: MockWebServer
    private val fake = FakeServer()
    private val cores = mutableListOf<StayaCore>()

    private fun leafFor(name: String) = HeldCertificate.Builder().signedBy(ca).addSubjectAlternativeName(name).build()

    private fun pin(cert: HeldCertificate): ByteArray =
        MessageDigest.getInstance("SHA-256").digest(cert.certificate.publicKey.encoded)

    /**
     * Сервер на заданном порту: подмена ключа «на том же адресе» поднимает новый
     * сервер сразу после закрытия старого, а порт освобождается не мгновенно
     * (в CI бывал `BindException`) — несколько попыток.
     */
    private fun startServer(cert: HeldCertificate, port: Int = 0) {
        var attempt = 0
        while (true) {
            server = MockWebServer()
            server.useHttps(HandshakeCertificates.Builder().heldCertificate(cert).build().sslSocketFactory())
            server.dispatcher = fake
            try {
                server.start(java.net.InetAddress.getByName("127.0.0.1"), port)
                return
            } catch (e: java.net.BindException) {
                server.close()
                if (++attempt >= 50) throw e
                Thread.sleep(100)
            }
        }
    }

    @Before
    fun setUp() = startServer(leaf)

    @After
    fun tearDown() {
        server.close()
        cores.forEach { it.close() }
    }

    private val port get() = server.url("/").port

    private fun core(pins: List<ByteArray>, host: String = "localhost:$port"): StayaCore {
        val path = Files.createTempDirectory("staya").resolve("staya.db").toString()
        return StayaCore.open(path, ByteArray(32) { 1 }).also {
            it.setServer(host, pins)
            cores += it
        }
    }

    private fun client(core: StayaCore, trustCa: Boolean = true): StayaClient {
        val trust = HandshakeCertificates.Builder().apply { if (trustCa) addTrustedCertificate(ca.certificate) }.build()
        return StayaClient(
            core,
            baseUrl = "https://localhost:$port",
            builder = OkHttpClient.Builder()
                .sslSocketFactory(trust.sslSocketFactory(), trust.trustManager)
                // Только адрес, на котором слушает MockWebServer: без перебора IPv6.
                .dns { listOf(java.net.InetAddress.getByName("127.0.0.1")) },
        )
    }

    private fun keyCount(c: StayaClient) = c.get("/v1/keys/count")

    // --- Правило доверия ---------------------------------------------------

    @Test
    fun primaryOrBackupPinIsAccepted() {
        for (pins in listOf(listOf(pin(leaf), ByteArray(32)), listOf(ByteArray(32), pin(leaf)))) {
            assertTrue(keyCount(client(core(pins))).contains("one_time_keys"))
        }
    }

    @Test
    fun wrongPinSendsNothing() {
        val c = client(core(listOf(pin(otherLeaf))))
        expect<ServerKeyRejectedException> { keyCount(c) }
        assertEquals("no request may reach a server with an unpinned key", 0, server.requestCount)
    }

    @Test
    fun tofuLearnsOnceThenRejectsAnotherKey() {
        val core = core(emptyList())
        keyCount(client(core))
        assertTrue(core.server()!!.learnedPin!!.contentEquals(pin(leaf)))

        val samePort = port
        server.close()
        startServer(otherLeaf, samePort)
        expect<ServerKeyRejectedException> { keyCount(client(core)) }
        assertEquals(0, server.requestCount)
        assertTrue("learned key is never replaced", core.server()!!.learnedPin!!.contentEquals(pin(leaf)))
    }

    @Test
    fun nameMismatchFailsBeforeAnyKeyIsLearned() {
        server.close()
        startServer(leafFor("other.example"))
        val core = core(emptyList())
        expect<SSLException> { keyCount(client(core)) }
        assertEquals(0, server.requestCount)
        assertNull(core.server()!!.learnedPin)
    }

    @Test
    fun untrustedChainFailsBeforeAnyKeyIsLearned() {
        val core = core(emptyList())
        expect<SSLException> { keyCount(client(core, trustCa = false)) }
        assertEquals(0, server.requestCount)
        assertNull(core.server()!!.learnedPin)
    }

    @Test
    fun webSocketGoesThroughTheSameKeyCheck() {
        val core = core(emptyList())
        keyCount(client(core)) // вход и запоминание ключа

        val samePort = port
        server.close()
        startServer(otherLeaf, samePort)
        val failed = java.util.concurrent.CountDownLatch(1)
        var error: Throwable? = null
        client(core).openWebSocket(object : okhttp3.WebSocketListener() {
            override fun onFailure(webSocket: okhttp3.WebSocket, t: Throwable, response: okhttp3.Response?) {
                error = t
                failed.countDown()
            }
        })
        assertTrue(failed.await(10, java.util.concurrent.TimeUnit.SECONDS))
        assertTrue("$error", error is javax.net.ssl.SSLPeerUnverifiedException)
        assertEquals("the token must not reach the server", 0, server.requestCount)
    }

    // --- Вход и повторы ----------------------------------------------------

    @Test
    fun logsInOnceAndReusesTheToken() {
        val c = client(core(listOf(pin(leaf))))
        keyCount(c)
        keyCount(c)
        assertEquals(1, fake.verifies.get())
        assertEquals(0, fake.registers.get())
    }

    @Test
    fun revokedTokenLogsInAgainOnce() {
        val c = client(core(listOf(pin(leaf))))
        keyCount(c)
        fake.revokeTokens()
        keyCount(c)
        assertEquals(2, fake.verifies.get())

        // Сервер отвергает и свежий токен — второго повтора нет.
        fake.alwaysUnauthorized = true
        expect<HttpStatusException> { keyCount(c) }
        assertEquals(3, fake.verifies.get())
    }

    @Test
    fun unknownAccountRegistersWithTheSameKeys() {
        fake.knownAccount = false
        val c = client(core(listOf(pin(leaf))))
        keyCount(c)
        assertEquals(1, fake.registers.get())
        assertEquals(1, fake.verifies.get())
    }

    @Test
    fun closedServerNeedsAnInviteCode() {
        fake.knownAccount = false
        fake.registerStatus = 403
        expect<InviteCodeRequiredException> { keyCount(client(core(listOf(pin(leaf))))) }
    }

    @Test
    fun rateLimitIsNotRetried() {
        val c = client(core(listOf(pin(leaf))))
        keyCount(c)
        val before = server.requestCount
        fake.rateLimited = true
        expect<RateLimitedException> { keyCount(c) }
        assertEquals(before + 1, server.requestCount)
    }

    private inline fun <reified T : Throwable> expect(block: () -> Unit) {
        try {
            block()
            fail("expected ${T::class.simpleName}")
        } catch (e: Throwable) {
            if (e !is T) throw AssertionError("expected ${T::class.simpleName}, got $e", e)
        }
    }

    /** Сервер с ответами по §4.1–4.2 и §8.3 без проверки подписей. */
    private class FakeServer : Dispatcher() {
        val verifies = AtomicInteger()
        val registers = AtomicInteger()
        @Volatile var knownAccount = true
        @Volatile var registerStatus = 201
        @Volatile var alwaysUnauthorized = false
        @Volatile var rateLimited = false
        private val tokens = java.util.concurrent.ConcurrentHashMap.newKeySet<String>()

        fun revokeTokens() = tokens.clear()

        private fun json(code: Int, body: String = "") =
            MockResponse.Builder().code(code).addHeader("Content-Type", "application/json").body(body).build()

        private fun b64(n: Int): String = Base64.getEncoder().encodeToString(ByteArray(32) { (it + n).toByte() })

        override fun dispatch(request: RecordedRequest): MockResponse {
            if (rateLimited) return json(429)
            return when (request.url.encodedPath) {
                "/v1/accounts" -> {
                    registers.incrementAndGet()
                    if (registerStatus in 200..299) knownAccount = true
                    json(registerStatus)
                }
                "/v1/auth/challenge" -> if (knownAccount) json(200, """{"nonce":"${b64(0)}"}""") else json(404)
                "/v1/auth/verify" -> {
                    val n = verifies.incrementAndGet()
                    val token = b64(n)
                    tokens += token
                    json(200, """{"token":"$token","expires_at":4102444800}""")
                }
                else -> {
                    val token = request.headers["Authorization"]?.removePrefix("Bearer ")
                    if (alwaysUnauthorized || token !in tokens) json(401) else json(200, """{"one_time_keys":5}""")
                }
            }
        }
    }
}

class BackoffTest {
    @Test
    fun growsWithJitterAndRespectsRateLimits() {
        val b = Backoff(baseMs = 1_000, maxMs = 8_000, random = kotlin.random.Random(1))
        val delays = List(6) { b.next() }
        delays.forEachIndexed { i, d ->
            val cap = minOf(1_000L shl i, 8_000L)
            assertTrue("$i: $d", d in cap / 2..cap)
        }
        assertTrue(b.next(rateLimited = true) >= Backoff.RATE_LIMITED_MS)
        b.reset()
        assertTrue(b.next() <= 1_000)
    }
}
