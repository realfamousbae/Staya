package io.github.realfamousbae.staya.ui

import io.github.realfamousbae.staya.net.StayaClient
import java.net.InetAddress
import java.nio.file.Files
import java.security.MessageDigest
import java.util.Base64
import java.util.Collections
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
import org.junit.Before
import org.junit.Test
import uniffi.staya_core.StayaCore

/** Онбординг (задача 4.2) против сервера с настоящим TLS: ссылка, код, ошибки, повтор. */
class OnboardingTest {
    private val ca = HeldCertificate.Builder().certificateAuthority(0).build()
    private val leaf = HeldCertificate.Builder().signedBy(ca).addSubjectAlternativeName("localhost").build()
    private val server = MockWebServer()
    private val paths = Collections.synchronizedList(mutableListOf<String>())
    private val bodies = Collections.synchronizedList(mutableListOf<String>())
    @Volatile private var inviteCode: String? = null
    private lateinit var core: StayaCore

    @Before
    fun setUp() {
        server.useHttps(HandshakeCertificates.Builder().heldCertificate(leaf).build().sslSocketFactory())
        server.dispatcher = object : Dispatcher() {
            override fun dispatch(request: RecordedRequest): MockResponse {
                val path = request.url.encodedPath
                val body = request.body?.utf8() ?: ""
                paths += "${request.method} $path"
                bodies += body
                val token = Base64.getEncoder().encodeToString(ByteArray(32) { 9 })
                fun ok(json: String = "{}") = MockResponse.Builder().code(200).body(json).build()
                return when (path) {
                    "/v1/accounts" ->
                        if (inviteCode == null || body.contains("\"invite_code\":\"$inviteCode\"")) {
                            MockResponse.Builder().code(201).build()
                        } else {
                            MockResponse.Builder().code(403).build()
                        }
                    "/v1/auth/challenge" ->
                        if (paths.any { it == "POST /v1/accounts" } && registered()) {
                            ok("""{"nonce":"${Base64.getEncoder().encodeToString(ByteArray(32))}"}""")
                        } else {
                            MockResponse.Builder().code(404).build()
                        }
                    "/v1/auth/verify" -> ok("""{"token":"$token","expires_at":4102444800}""")
                    "/v1/keys/count" -> ok("""{"one_time_keys":0}""")
                    else -> MockResponse.Builder().code(204).build()
                }
            }
        }
        server.start(InetAddress.getByName("127.0.0.1"), 0)
        val path = Files.createTempDirectory("staya").resolve("staya.db").toString()
        core = StayaCore.open(path, ByteArray(32) { 2 })
    }

    /** Регистрация удалась хотя бы раз (201). */
    private fun registered(): Boolean = synchronized(paths) {
        paths.indices.any { paths[it] == "POST /v1/accounts" && (inviteCode == null || bodies[it].contains(inviteCode!!)) }
    }

    @After
    fun tearDown() {
        server.close()
        core.close()
    }

    private val host get() = "localhost:${server.url("/").port}"

    private fun pin(): String = Base64.getUrlEncoder().withoutPadding()
        .encodeToString(MessageDigest.getInstance("SHA-256").digest(leaf.certificate.publicKey.encoded))

    private fun clientFactory(): (StayaCore, () -> String?) -> StayaClient = { c, code ->
        val trust = HandshakeCertificates.Builder().addTrustedCertificate(ca.certificate).build()
        StayaClient(
            c,
            inviteCode = code,
            builder = OkHttpClient.Builder()
                .sslSocketFactory(trust.sslSocketFactory(), trust.trustManager)
                .dns { listOf(InetAddress.getByName("127.0.0.1")) },
        )
    }

    private fun onboard(link: String = "", manual: String = "", code: String = "", nick: String = "Лёша") =
        AppModel.onboard(core, link, manual, code, nick, null, clientFactory())

    @Test
    fun serverLinkWithPinRegistersAndPublishesKeys() {
        assertNull(onboard(link = "staya://server?v=1&s=$host&p=${pin()}"))
        assertEquals(host, core.server()!!.host)
        assertEquals("Лёша", core.myProfile().nick)
        assertTrue(paths.toString(), "PUT /v1/keys" in paths)
    }

    @Test
    fun closedServerAsksForCodeThenWorks() {
        inviteCode = "beta"
        val message = onboard(link = "staya://server?v=1&s=$host&p=${pin()}")!!
        assertTrue(message, message.contains("код приглашения"))
        assertNull("binding is undone so the user can retry", core.server())
        assertNull(onboard(link = "staya://server?v=1&s=$host&p=${pin()}", code = " beta "))
    }

    @Test
    fun manualServerWithoutPinLearnsTheKey() {
        assertNull(onboard(manual = host))
        assertTrue(core.server()!!.learnedPin!!.isNotEmpty())
    }

    @Test
    fun wrongPinIsReportedAndNothingIsSent() {
        val wrong = Base64.getUrlEncoder().withoutPadding().encodeToString(ByteArray(32))
        val message = onboard(link = "staya://server?v=1&s=$host&p=$wrong")!!
        assertTrue(message, message.contains("Ключ сервера"))
        assertTrue(paths.toString(), paths.isEmpty())
        assertNull(core.server())
    }

    @Test
    fun noServerAndBadLinkAreExplained() {
        assertTrue(onboard()!!.contains("приглашение"))
        assertTrue(onboard(link = "staya://server?v=1")!!.contains("разобрать ссылку"))
    }
}
