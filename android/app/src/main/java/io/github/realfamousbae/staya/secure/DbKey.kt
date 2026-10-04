package io.github.realfamousbae.staya.secure

import java.security.SecureRandom

/** Результат чтения секрета из хранилища. */
sealed interface SecretRead {
    class Found(val value: ByteArray) : SecretRead
    data object NotFound : SecretRead
    /** Хранилище временно недоступно (например, сразу после загрузки) — повторить позже. */
    class Unavailable(val cause: Exception) : SecretRead
    /** Секрет есть, но не расшифровывается. */
    class Broken(val cause: Exception) : SecretRead
}

/** Хранилище одного секрета; в тестах — подделка. */
interface KeyBackend {
    fun read(): SecretRead
    fun addIfAbsent(value: ByteArray)
}

/**
 * Получение ключа локальной базы по правилам docs/protocol.md §3.1. Главное правило:
 * новый ключ создаётся только когда старого точно нет — при временной ошибке
 * хранилища новый ключ навсегда отрезал бы существующую базу.
 */
object DbKey {
    const val SIZE = 32

    sealed interface Result {
        class Ready(val key: ByteArray) : Result
        class Unavailable(val reason: String) : Result
        class Broken(val reason: String) : Result
    }

    fun obtain(
        store: KeyBackend,
        dbExists: () -> Boolean,
        deleteDb: () -> Unit,
        newKey: () -> ByteArray = { ByteArray(SIZE).also(SecureRandom()::nextBytes) },
    ): Result {
        when (val r = store.read()) {
            is SecretRead.Found -> return ready(r.value)
            is SecretRead.Unavailable -> return Result.Unavailable(describe(r.cause))
            is SecretRead.Broken -> return Result.Broken(describe(r.cause))
            SecretRead.NotFound -> Unit
        }
        // Ключа точно нет: база без него нечитаема — удаляем её до того, как появится новый ключ.
        if (dbExists()) deleteDb()
        try {
            store.addIfAbsent(newKey())
        } catch (e: Exception) {
            return Result.Unavailable(describe(e))
        }
        // Читаем обратно: открываем базу только ключом, который действительно сохранён.
        return when (val r = store.read()) {
            is SecretRead.Found -> ready(r.value)
            SecretRead.NotFound -> Result.Unavailable("key was not saved")
            is SecretRead.Unavailable -> Result.Unavailable(describe(r.cause))
            is SecretRead.Broken -> Result.Broken(describe(r.cause))
        }
    }

    private fun ready(key: ByteArray): Result =
        if (key.size == SIZE) Result.Ready(key) else Result.Broken("key has ${key.size} bytes")

    private fun describe(e: Exception) = e.javaClass.simpleName + (e.message?.let { ": $it" } ?: "")
}
