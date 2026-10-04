package io.github.realfamousbae.staya.secure

import android.content.Context
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyPermanentlyInvalidatedException
import android.security.keystore.KeyProperties
import java.io.File
import java.io.IOException
import java.security.GeneralSecurityException
import java.security.KeyStore
import java.security.UnrecoverableKeyException
import javax.crypto.AEADBadTagException
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec

/**
 * Секреты в Android Keystore: неэкспортируемый AES-256-GCM-ключ, на диске только
 * шифротекст в `noBackupFilesDir` (не попадает в бэкап и перенос). Формат файла:
 * длина IV (1 байт) ‖ IV ‖ шифротекст. SharedPreferences для секретов не используем (CLAUDE.md).
 */
class SecretStore(context: Context, private val name: String) : KeyBackend {
    private val dir = context.noBackupFilesDir
    private val file = File(dir, "secret-$name.bin")

    override fun read(): SecretRead {
        val bytes = try {
            if (!file.exists()) return SecretRead.NotFound
            file.readBytes()
        } catch (e: IOException) {
            return SecretRead.Unavailable(e)
        }
        return try {
            val ivLen = bytes[0].toInt()
            val iv = bytes.copyOfRange(1, 1 + ivLen)
            val cipher = Cipher.getInstance(TRANSFORMATION)
            cipher.init(Cipher.DECRYPT_MODE, existingKey() ?: return SecretRead.Broken(missingKey()), GCMParameterSpec(128, iv))
            SecretRead.Found(cipher.doFinal(bytes, 1 + ivLen, bytes.size - 1 - ivLen))
        } catch (e: AEADBadTagException) {
            SecretRead.Broken(e)
        } catch (e: KeyPermanentlyInvalidatedException) {
            SecretRead.Broken(e)
        } catch (e: UnrecoverableKeyException) {
            SecretRead.Broken(e)
        } catch (e: IndexOutOfBoundsException) {
            SecretRead.Broken(e)
        } catch (e: IllegalArgumentException) {
            SecretRead.Broken(e)
        } catch (e: GeneralSecurityException) {
            // KeyStoreException и прочие сбои Keystore — бывают сразу после загрузки.
            SecretRead.Unavailable(e)
        } catch (e: java.security.ProviderException) {
            SecretRead.Unavailable(e)
        }
    }

    /** Записывает, только если секрета ещё нет: существующий никогда не перезаписывается. */
    override fun addIfAbsent(value: ByteArray) {
        if (file.exists()) return
        write(value)
    }

    /** Перезаписывает секрет (токен замеров). Для ключа базы — только [addIfAbsent]. */
    fun replace(value: ByteArray) = write(value)

    /** Удаляет и файл, и ключ Keystore (сброс после [SecretRead.Broken]). */
    fun delete() {
        file.delete()
        runCatching { KeyStore.getInstance(KEYSTORE).apply { load(null) }.deleteEntry(alias()) }
    }

    /** Атомарно: временный файл и переименование — оборванная запись не портит старый файл. */
    private fun write(value: ByteArray) {
        val cipher = Cipher.getInstance(TRANSFORMATION)
        cipher.init(Cipher.ENCRYPT_MODE, existingKey() ?: newKey())
        val ciphertext = cipher.doFinal(value)
        val tmp = File(dir, "secret-$name.tmp")
        tmp.outputStream().use { out ->
            out.write(byteArrayOf(cipher.iv.size.toByte()) + cipher.iv + ciphertext)
            out.fd.sync()
        }
        if (!tmp.renameTo(file)) throw IOException("rename failed")
    }

    private fun existingKey(): SecretKey? {
        val ks = KeyStore.getInstance(KEYSTORE).apply { load(null) }
        return (ks.getEntry(alias(), null) as? KeyStore.SecretKeyEntry)?.secretKey
    }

    /** StrongBox, если получится; на части устройств он падает не только StrongBoxUnavailableException. */
    private fun newKey(): SecretKey = try {
        generate(strongBox = true)
    } catch (e: Exception) {
        generate(strongBox = false)
    }

    private fun generate(strongBox: Boolean): SecretKey {
        val gen = KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, KEYSTORE)
        gen.init(
            KeyGenParameterSpec.Builder(alias(), KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT)
                .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
                .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
                .setKeySize(256)
                .setIsStrongBoxBacked(strongBox)
                // Без setUnlockedDeviceRequired: ключ нужен и при заблокированном экране —
                // фоновые отправки (как AfterFirstUnlockThisDeviceOnly на iOS).
                .build(),
        )
        return gen.generateKey()
    }

    private fun missingKey() = UnrecoverableKeyException("Keystore key $name is gone")

    private fun alias() = "staya.$name"

    private companion object {
        const val KEYSTORE = "AndroidKeyStore"
        const val TRANSFORMATION = "AES/GCM/NoPadding"
    }
}
