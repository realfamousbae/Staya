package io.github.realfamousbae.staya.map

import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.graphics.Canvas
import android.graphics.Paint
import android.graphics.PorterDuff
import android.graphics.PorterDuffXfermode
import android.graphics.Rect
import android.graphics.RectF
import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableLongStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.viewinterop.AndroidView
import androidx.core.graphics.createBitmap
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.LifecycleEventObserver
import androidx.lifecycle.compose.LocalLifecycleOwner
import java.util.concurrent.Executors
import kotlinx.coroutines.delay
import org.maplibre.android.MapLibre
import org.maplibre.android.camera.CameraUpdateFactory
import org.maplibre.android.geometry.LatLng
import org.maplibre.android.geometry.LatLngBounds
import org.maplibre.android.maps.MapLibreMap
import org.maplibre.android.maps.MapView
import org.maplibre.android.maps.Style
import org.maplibre.android.style.expressions.Expression
import org.maplibre.android.style.layers.FillLayer
import org.maplibre.android.style.layers.LineLayer
import org.maplibre.android.style.layers.PropertyFactory
import org.maplibre.android.style.layers.SymbolLayer
import org.maplibre.android.style.sources.GeoJsonSource
import org.maplibre.geojson.Feature
import org.maplibre.geojson.FeatureCollection
import org.maplibre.geojson.Point
import org.maplibre.geojson.Polygon
import uniffi.staya_core.FriendView
import uniffi.staya_core.LocationKind
import uniffi.staya_core.StayaCore

/** Куда сдвинуть карту: друг из списка (по нажатию). */
class MapFocus(val friendId: String, val nonce: Long = System.nanoTime())

/**
 * Карта друзей (задача 4.4): стиль и тайлы со своего сервера, маркер — аватар и
 * «ник · N мин назад», круг точности. Скрывшие позицию на карте не показываются.
 */
@Composable
fun FriendsMap(core: StayaCore, host: String, friends: List<FriendView>, focus: MapFocus?, modifier: Modifier = Modifier) {
    val context = LocalContext.current
    val dark = isSystemInDarkTheme()
    val density = LocalDensity.current.density
    val lifecycle = LocalLifecycleOwner.current.lifecycle
    var map by remember { mutableStateOf<MapLibreMap?>(null) }
    var style by remember { mutableStateOf<Style?>(null) }
    var now by remember { mutableLongStateOf(System.currentTimeMillis() / 1000) }
    // 0 — камера не выставлена, 1 — показан регион, 2 — показаны друзья.
    var fitted by remember { mutableIntStateOf(0) }
    // Ответ с границами региона может прийти после показа друзей или после закрытия карты.
    val alive = remember { java.util.concurrent.atomic.AtomicBoolean(true) }

    val mapView = remember {
        MapLibre.getInstance(context.applicationContext)
        MapNetwork.install(core)
        MapView(context).apply { onCreate(null) }
    }

    // MapView требует все события жизненного цикла, иначе утечки и падения.
    DisposableEffect(lifecycle, mapView) {
        val observer = LifecycleEventObserver { _, event ->
            when (event) {
                Lifecycle.Event.ON_START -> mapView.onStart()
                Lifecycle.Event.ON_RESUME -> mapView.onResume()
                Lifecycle.Event.ON_PAUSE -> mapView.onPause()
                Lifecycle.Event.ON_STOP -> mapView.onStop()
                else -> {}
            }
        }
        lifecycle.addObserver(observer)
        onDispose {
            alive.set(false)
            lifecycle.removeObserver(observer)
            if (lifecycle.currentState.isAtLeast(Lifecycle.State.RESUMED)) mapView.onPause()
            if (lifecycle.currentState.isAtLeast(Lifecycle.State.STARTED)) mapView.onStop()
            mapView.onDestroy()
        }
    }

    LaunchedEffect(mapView, host, dark) {
        style = null
        mapView.getMapAsync { m ->
            map = m
            m.uiSettings.isRotateGesturesEnabled = false
            m.setStyle(Style.Builder().fromUri(MapNetwork.styleUrl(host, dark))) { s ->
                addLayers(s, dark)
                style = s
            }
        }
    }

    // Подписи «N мин назад» устаревают: обновляем раз в минуту, пока карта на экране.
    LaunchedEffect(Unit) {
        while (true) {
            delay(60_000)
            now = System.currentTimeMillis() / 1000
        }
    }

    LaunchedEffect(style, friends, now) {
        val s = style ?: return@LaunchedEffect
        updateFriends(density, s, friends, now)
        val m = map ?: return@LaunchedEffect
        // Список друзей приходит после карты: сначала регион, потом — друзья.
        val hasVisible = friends.any { f -> f.location?.let { it.kind != LocationKind.HIDDEN } == true }
        if (hasVisible && fitted < 2) {
            fitted = 2
            fitInitial(m, host, friends) { false }
        } else if (fitted == 0) {
            fitted = 1
            fitInitial(m, host, friends) { alive.get() && fitted == 1 }
        }
    }

    LaunchedEffect(focus, map) {
        val f = focus ?: return@LaunchedEffect
        val loc = friends.firstOrNull { it.accountId == f.friendId }?.location ?: return@LaunchedEffect
        if (loc.kind == LocationKind.HIDDEN) return@LaunchedEffect
        map?.animateCamera(CameraUpdateFactory.newLatLngZoom(LatLng(loc.latE7 / 1e7, loc.lonE7 / 1e7), 15.0))
    }

    AndroidView(factory = { mapView }, modifier = modifier)
}

private const val SOURCE_FRIENDS = "staya-friends"
private const val SOURCE_ACCURACY = "staya-accuracy"

private fun addLayers(style: Style, dark: Boolean) {
    style.addSource(GeoJsonSource(SOURCE_ACCURACY))
    style.addSource(GeoJsonSource(SOURCE_FRIENDS))
    style.addLayer(
        FillLayer("staya-accuracy-fill", SOURCE_ACCURACY).withProperties(
            PropertyFactory.fillColor("#2F6FED"),
            PropertyFactory.fillOpacity(0.12f),
        ),
    )
    style.addLayer(
        LineLayer("staya-accuracy-line", SOURCE_ACCURACY).withProperties(
            PropertyFactory.lineColor("#2F6FED"),
            PropertyFactory.lineOpacity(0.5f),
            PropertyFactory.lineWidth(1f),
        ),
    )
    style.addLayer(
        SymbolLayer("staya-friends", SOURCE_FRIENDS).withProperties(
            PropertyFactory.iconImage(Expression.get("icon")),
            PropertyFactory.iconAllowOverlap(true),
            PropertyFactory.iconIgnorePlacement(true),
            PropertyFactory.textField(Expression.get("label")),
            // Шрифты — только те, что раздаёт сервер (deploy/map/update-map.sh).
            PropertyFactory.textFont(arrayOf("Noto Sans Medium")),
            PropertyFactory.textSize(12f),
            PropertyFactory.textAnchor("top"),
            PropertyFactory.textOffset(arrayOf(0f, 1.9f)),
            PropertyFactory.textAllowOverlap(true),
            PropertyFactory.textColor(if (dark) "#F2F2F5" else "#1B1B1F"),
            PropertyFactory.textHaloColor(if (dark) "#1B1B1F" else "#FFFFFF"),
            PropertyFactory.textHaloWidth(1.5f),
        ),
    )
}

private fun updateFriends(density: Float, style: Style, friends: List<FriendView>, now: Long) {
    val points = mutableListOf<Feature>()
    val circles = mutableListOf<Feature>()
    for (f in friends) {
        val loc = f.location ?: continue
        if (loc.kind == LocationKind.HIDDEN) continue
        // Имя картинки меняется вместе с аватаром и ником — новая картинка, а не старая из кэша стиля.
        val icon = "avatar-" + f.accountId + "-" + (f.avatar?.contentHashCode() ?: 0) + "-" + f.nick.hashCode()
        if (style.getImage(icon) == null) style.addImage(icon, markerBitmap(f, density))
        val point = Point.fromLngLat(loc.lonE7 / 1e7, loc.latE7 / 1e7)
        points += Feature.fromGeometry(point).apply {
            addStringProperty("icon", icon)
            addStringProperty("label", markerLabel(f, now))
        }
        if (loc.accuracyM.toInt() > 0) {
            val ring = MapMath.accuracyRing(loc.latE7, loc.lonE7, loc.accuracyM.toDouble())
                .map { Point.fromLngLat(it[0], it[1]) }
            circles += Feature.fromGeometry(Polygon.fromLngLats(listOf(ring)))
        }
    }
    style.getSourceAs<GeoJsonSource>(SOURCE_FRIENDS)?.setGeoJson(FeatureCollection.fromFeatures(points))
    style.getSourceAs<GeoJsonSource>(SOURCE_ACCURACY)?.setGeoJson(FeatureCollection.fromFeatures(circles))
}

/** Подпись маркера и строка в списке: где друг и насколько свежо. */
fun locationStatus(f: FriendView, now: Long): String? {
    val loc = f.location ?: return null
    val age = MapMath.ageLabel(now, loc.timestamp)
    return when (loc.kind) {
        LocationKind.EXACT -> age
        LocationKind.APPROX -> "примерно · $age"
        LocationKind.FROZEN -> "позиция заморожена · $age"
        LocationKind.HIDDEN -> "скрыл(а) позицию"
    }
}

private fun markerLabel(f: FriendView, now: Long): String =
    (f.nick ?: "Без имени") + "\n" + (locationStatus(f, now) ?: "")

/** Круглый аватар с обводкой; без аватара — первая буква ника. */
private fun markerBitmap(f: FriendView, density: Float): Bitmap {
    val size = (44 * density).toInt()
    val border = 3 * density
    val out = createBitmap(size, size)
    val canvas = Canvas(out)
    val paint = Paint(Paint.ANTI_ALIAS_FLAG)
    paint.color = 0xFFFFFFFF.toInt()
    canvas.drawCircle(size / 2f, size / 2f, size / 2f, paint)
    val inner = RectF(border, border, size - border, size - border)
    val avatar = f.avatar?.takeIf { it.isNotEmpty() }?.let { BitmapFactory.decodeByteArray(it, 0, it.size) }
    if (avatar != null) {
        val layer = canvas.saveLayer(inner, null)
        paint.color = 0xFF000000.toInt()
        canvas.drawOval(inner, paint)
        paint.xfermode = PorterDuffXfermode(PorterDuff.Mode.SRC_IN)
        canvas.drawBitmap(avatar, Rect(0, 0, avatar.width, avatar.height), inner, paint)
        paint.xfermode = null
        canvas.restoreToCount(layer)
    } else {
        paint.color = 0xFF2F6FED.toInt()
        canvas.drawOval(inner, paint)
        paint.color = 0xFFFFFFFF.toInt()
        paint.textSize = size * 0.45f
        paint.textAlign = Paint.Align.CENTER
        val letter = f.nick?.trim()?.firstOrNull()?.uppercase() ?: "?"
        val y = size / 2f - (paint.descent() + paint.ascent()) / 2
        canvas.drawText(letter, size / 2f, y, paint)
    }
    return out
}

private val boundsWorker = Executors.newSingleThreadExecutor()

/** Первый показ: все друзья на карте; если их нет — регион карты сервера. */
private fun fitInitial(map: MapLibreMap, host: String, friends: List<FriendView>, stillRegion: () -> Boolean) {
    val visible = friends.mapNotNull { it.location }.filter { it.kind != LocationKind.HIDDEN }
    when {
        visible.size == 1 -> map.moveCamera(
            CameraUpdateFactory.newLatLngZoom(LatLng(visible[0].latE7 / 1e7, visible[0].lonE7 / 1e7), 14.0),
        )
        visible.size > 1 -> {
            val bounds = LatLngBounds.Builder()
            visible.forEach { bounds.include(LatLng(it.latE7 / 1e7, it.lonE7 / 1e7)) }
            map.moveCamera(CameraUpdateFactory.newLatLngBounds(bounds.build(), 160))
        }
        else -> boundsWorker.execute {
            val b = MapNetwork.regionBounds(host) ?: return@execute
            val region = LatLngBounds.from(b[3], b[2], b[1], b[0])
            android.os.Handler(android.os.Looper.getMainLooper()).post {
                if (stillRegion()) map.moveCamera(CameraUpdateFactory.newLatLngBounds(region, 0))
            }
        }
    }
}
