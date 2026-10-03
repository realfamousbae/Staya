package io.github.realfamousbae.staya.probe

import android.content.Context
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import java.io.File
import java.security.KeyStore
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec

/**
 * Секреты в Android Keystore: неэкспортируемый AES-GCM-ключ, на диске только
 * шифротекст в `noBackupFilesDir` (не попадает в бэкап и перенос). Основа для
 * ключа базы в задаче 2.9. SharedPreferences для секретов не используем (CLAUDE.md).
 */
class SecretStore(context: Context, private val name: String) {
    private val file = File(context.noBackupFilesDir, "secret-$name.bin")

    fun save(value: String) {
        val cipher = Cipher.getInstance(TRANSFORMATION)
        cipher.init(Cipher.ENCRYPT_MODE, key())
        val ciphertext = cipher.doFinal(value.toByteArray(Charsets.UTF_8))
        file.writeBytes(byteArrayOf(cipher.iv.size.toByte()) + cipher.iv + ciphertext)
    }

    fun load(): String? {
        if (!file.exists()) return null
        return runCatching {
            val bytes = file.readBytes()
            val ivLen = bytes[0].toInt()
            val iv = bytes.copyOfRange(1, 1 + ivLen)
            val cipher = Cipher.getInstance(TRANSFORMATION)
            cipher.init(Cipher.DECRYPT_MODE, key(), GCMParameterSpec(128, iv))
            String(cipher.doFinal(bytes.copyOfRange(1 + ivLen, bytes.size)), Charsets.UTF_8)
        }.getOrNull()
    }

    private fun key(): SecretKey {
        val ks = KeyStore.getInstance(KEYSTORE).apply { load(null) }
        (ks.getEntry(alias(), null) as? KeyStore.SecretKeyEntry)?.let { return it.secretKey }
        val gen = KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, KEYSTORE)
        gen.init(
            KeyGenParameterSpec.Builder(alias(), KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT)
                .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
                .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
                .setKeySize(256)
                // Доступен и при заблокированном экране — фоновые отправки (как
                // AfterFirstUnlockThisDeviceOnly на iOS).
                .build(),
        )
        return gen.generateKey()
    }

    private fun alias() = "staya.$name"

    private companion object {
        const val KEYSTORE = "AndroidKeyStore"
        const val TRANSFORMATION = "AES/GCM/NoPadding"
    }
}
