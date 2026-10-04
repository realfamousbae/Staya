package io.github.realfamousbae.staya.net

import java.security.MessageDigest
import java.security.cert.X509Certificate
import javax.net.ssl.HostnameVerifier
import javax.net.ssl.SSLSession
import uniffi.staya_core.ServerTrust

/**
 * Правило доверия к серверу (protocol §5.3) на этапе установки TLS-соединения —
 * до того как OkHttp отправит первый байт запроса (в том числе токен), и одинаково
 * для HTTP и WebSocket.
 *
 * Сначала обычная проверка имени (цепочку уже проверил системный TrustManager),
 * затем SHA-256 от SubjectPublicKeyInfo ключа сервера отдаётся ядру: оно сверяет
 * с отпечатками или ключом, запомненным при первом подключении.
 */
class ServerKeyVerifier(
    private val default: HostnameVerifier,
    private val check: (spkiSha256: ByteArray) -> ServerTrust,
) : HostnameVerifier {
    /**
     * Соединение в этом потоке отвергнуто из-за ключа (а не имени): для понятной
     * ошибки. По потоку: синхронные вызовы OkHttp устанавливают соединение в
     * вызывающем потоке, а наружу может выйти ошибка другого адреса того же имени.
     */
    private val rejected = ThreadLocal.withInitial { false }

    fun resetRejected() = rejected.set(false)

    fun keyRejected(): Boolean = rejected.get()

    override fun verify(hostname: String, session: SSLSession): Boolean {
        if (!default.verify(hostname, session)) return false
        val leaf = session.peerCertificates.firstOrNull() as? X509Certificate ?: return false
        val spki = MessageDigest.getInstance("SHA-256").digest(leaf.publicKey.encoded)
        val trusted = runCatching { check(spki) }.getOrDefault(ServerTrust.REJECTED) != ServerTrust.REJECTED
        if (!trusted) rejected.set(true)
        return trusted
    }
}
