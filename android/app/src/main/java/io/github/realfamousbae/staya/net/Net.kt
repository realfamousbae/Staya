package io.github.realfamousbae.staya.net

import java.util.concurrent.Executors
import java.util.concurrent.ScheduledExecutorService
import uniffi.staya_core.StayaCore

/**
 * Единственный на процесс владелец сети к серверу Staya: клиент, [CoreSync] и
 * поток [worker]. Через этот поток идут все изменения ядра, связанные с отправкой
 * (в том числе `prepareLocationUpdate`), сеть экрана ([LiveConnection]) и фоновая
 * отправка позиции — так исходящая очередь ядра не обрабатывается параллельно.
 */
object Net {
    val worker: ScheduledExecutorService = Executors.newSingleThreadScheduledExecutor()

    private var core: StayaCore? = null
    private var client: StayaClient? = null
    private var sync: CoreSync? = null

    /**
     * Клиент и синхронизация для ядра; только из [worker]. `null` — аккаунт ещё
     * не привязан к серверу (онбординг не закончен).
     */
    fun bind(core: StayaCore): Pair<StayaClient, CoreSync>? {
        if (this.core !== core || client == null) {
            val c = runCatching { StayaClient(core) }.getOrNull() ?: return null
            this.core = core
            client = c
            sync = CoreSync(core, c)
        }
        return client!! to sync!!
    }
}
