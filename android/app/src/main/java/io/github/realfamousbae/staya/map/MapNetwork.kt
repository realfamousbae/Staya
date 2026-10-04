package io.github.realfamousbae.staya.map

import io.github.realfamousbae.staya.net.ServerKeyVerifier
import java.io.IOException
import java.util.concurrent.TimeUnit
import okhttp3.Dispatcher
import okhttp3.HttpUrl
import okhttp3.HttpUrl.Companion.toHttpUrlOrNull
import okhttp3.Interceptor
import okhttp3.OkHttpClient
import okhttp3.Request
import org.maplibre.android.log.Logger
import org.maplibre.android.module.http.HttpRequestUtil
import uniffi.staya_core.StayaCore

/**
 * Сеть карты: все запросы MapLibre (стиль, тайлы, шрифты, спрайты) — только https
 * к серверу, к которому привязан аккаунт, и с той же проверкой ключа сервера, что
 * у API (protocol §5.3). Иначе подменивший сервер по дороге видел бы, какой участок
 * карты смотрят — то есть примерно где друзья.
 *
 * Журналы MapLibre выключены: адреса тайлов (z/x/y) выдают просматриваемое место.
 */
object MapNetwork {
    @Volatile
    private var installedFor: StayaCore? = null

    /** Ставит клиент один раз на ядро; адрес сервера читается из ядра на каждый запрос. */
    @Synchronized
    fun install(core: StayaCore): OkHttpClient? {
        if (installedFor === core) return client
        val host = { runCatching { core.server()?.host }.getOrNull() }
        val base = OkHttpClient.Builder()
            .connectTimeout(15, TimeUnit.SECONDS)
            .readTimeout(30, TimeUnit.SECONDS)
            // Тайлы грузятся параллельно (у OkHttp по умолчанию 5 на сервер — медленно).
            .dispatcher(Dispatcher().apply { maxRequestsPerHost = 20 })
            .addInterceptor(Interceptor { chain ->
                val url = chain.request().url
                if (!allowed(url, host())) throw IOException("map request outside the bound server")
                chain.proceed(chain.request())
            })
            .build()
        val pinned = base.newBuilder()
            .hostnameVerifier(ServerKeyVerifier(base.hostnameVerifier) { core.checkServerKey(it) })
            .build()
        HttpRequestUtil.setLogEnabled(false)
        HttpRequestUtil.setPrintRequestUrlOnFailure(false)
        HttpRequestUtil.setOkHttpClient(pinned)
        Logger.setVerbosity(Logger.NONE)
        client = pinned
        installedFor = core
        return pinned
    }

    @Volatile
    private var client: OkHttpClient? = null

    /** Адрес стиля на сервере: `https://<сервер>/map/style-light.json` (или dark). */
    fun styleUrl(host: String, dark: Boolean): String =
        "https://$host/map/style-${if (dark) "dark" else "light"}.json"

    /** Границы региона карты из TileJSON сервера; блокирующий вызов. */
    fun regionBounds(host: String): DoubleArray? {
        val http = client ?: return null
        val request = Request.Builder().url("https://$host/tiles/region.json").build()
        return runCatching {
            http.newCall(request).execute().use { r ->
                if (r.isSuccessful) MapMath.tileJsonBounds(r.body.string()) else null
            }
        }.getOrNull()
    }

    /** Только https и только привязанный сервер (имя и порт). */
    fun allowed(url: HttpUrl, host: String?): Boolean {
        val bound = host?.let { "https://$it/".toHttpUrlOrNull() } ?: return false
        return url.isHttps && url.host == bound.host && url.port == bound.port
    }
}
