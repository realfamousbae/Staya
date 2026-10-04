package io.github.realfamousbae.staya

import android.content.Context
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import io.github.realfamousbae.staya.secure.DbKey
import io.github.realfamousbae.staya.secure.SecretStore
import java.io.File
import java.util.concurrent.Executors
import uniffi.staya_core.CoreException
import uniffi.staya_core.StayaCore

/**
 * Единственный на процесс экземпляр ядра (база открывается эксклюзивно). Открывается
 * лениво и не на главном потоке: Keystore (особенно StrongBox) и SQLite медленные.
 */
object AppCore {
    sealed interface State {
        data object Closed : State
        data object Opening : State
        class Open(val core: StayaCore, val accountId: String) : State
        /** Временная ошибка — откроется при следующей попытке. */
        class Unavailable(val reason: String) : State
        /** Ключ или база испорчены — только явный сброс. */
        class Broken(val reason: String) : State
    }

    private val executor = Executors.newSingleThreadExecutor()

    var state by mutableStateOf<State>(State.Closed)
        private set

    /** Открывает ядро в фоне, если оно ещё не открыто. */
    fun openAsync(context: Context) {
        val app = context.applicationContext
        executor.execute { if (state !is State.Open) state = guarded { open(app) } }
    }

    /**
     * Для фоновой работы без экрана (сервис геопозиции): открыть и дождаться.
     * Недоступная или испорченная база — `null`, никакого сброса: его делает только
     * пользователь. Не вызывать из главного потока.
     */
    fun openBlocking(context: Context): StayaCore? {
        (state as? State.Open)?.let { return it.core }
        val app = context.applicationContext
        val result = executor.submit<State> {
            if (state !is State.Open) state = guarded { open(app) }
            state
        }.get()
        return (result as? State.Open)?.core
    }

    /** Удаляет базу и ключ и создаёт аккаунт заново. Друзей придётся добавить снова. */
    fun resetAsync(context: Context) {
        val app = context.applicationContext
        executor.execute {
            state = guarded {
                (state as? State.Open)?.core?.close()
                deleteDb(app)
                SecretStore(app, KEY_NAME).delete()
                open(app)
            }
        }
    }

    /**
     * Любой сбой (Keystore, загрузка нативной библиотеки) — состояние, а не падение:
     * этот APK ставят друзья для замеров, падение на потоке executor убило бы процесс.
     */
    private inline fun guarded(block: () -> State): State = try {
        block()
    } catch (e: Throwable) {
        State.Unavailable(e.javaClass.simpleName)
    }

    private fun open(app: Context): State {
        state = State.Opening
        val db = dbFile(app)
        val key = when (val r = DbKey.obtain(SecretStore(app, KEY_NAME), db::exists, { deleteDb(app) })) {
            is DbKey.Result.Ready -> r.key
            is DbKey.Result.Unavailable -> return State.Unavailable(r.reason)
            is DbKey.Result.Broken -> return State.Broken(r.reason)
        }
        return try {
            val core = StayaCore.open(db.path, key)
            State.Open(core, core.identity().accountId)
        } catch (e: CoreException.Corrupted) {
            State.Broken(e.message ?: "corrupted")
        } catch (e: CoreException) {
            State.Unavailable(e.message ?: e.javaClass.simpleName)
        } finally {
            key.fill(0)
        }
    }

    private fun dbFile(app: Context) = File(app.noBackupFilesDir, "staya.db")

    private fun deleteDb(app: Context) {
        val db = dbFile(app)
        File(db.path + "-journal").delete()
        db.delete()
    }

    private const val KEY_NAME = "db-key"
}
