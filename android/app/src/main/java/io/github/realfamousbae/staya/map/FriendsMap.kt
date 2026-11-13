package io.github.realfamousbae.staya.map

import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.graphics.Canvas
import android.graphics.Paint
import android.graphics.PorterDuff
import android.graphics.PorterDuffXfermode
import android.graphics.Rect
import android.graphics.RectF
import android.graphics.Typeface
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

/** Куда сдвинуть карту: друг из списка (по нажатию) или все друзья (`friendId == null`). */
class MapFocus(val friendId: String?, val nonce: Long = System.nanoTime())

/**
 * Карта друзей (задачи 4.4, 5.2): стиль и тайлы со своего сервера; маркер — аватар в
 * белом кольце с бейджем давности под ним (как в Blink, но вместо заряда — «5 мин»),
 * круг точности. Скрывшие позицию на карте не показываются. Кнопок на карте нет —
 * они у экрана поверх неё.
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
    // Картинки маркеров по друзьям: бейдж меняется раз в минуту, старые убираем из стиля.
    val icons = remember { mutableMapOf<String, String>() }

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
        icons.clear()
        mapView.getMapAsync { m ->
            map = m
            m.uiSettings.isRotateGesturesEnabled = false
            // Логотип движка не нужен; атрибуция данных карты (OpenStreetMap) обязательна —
            // кнопка «i» слева снизу, над нижними кнопками экрана не мешает.
            m.uiSettings.isLogoEnabled = false
            m.uiSettings.setAttributionMargins((8 * density).toInt(), 0, 0, (96 * density).toInt())
            m.uiSettings.setAttributionTintColor(if (dark) 0xFFFFFFFF.toInt() else 0xFF0F0F12.toInt())
            m.setStyle(Style.Builder().fromUri(MapNetwork.styleUrl(host, dark))) { s ->
                addLayers(s)
                style = s
            }
        }
    }

    // Бейджи «N мин» устаревают: обновляем раз в минуту, пока карта на экране.
    LaunchedEffect(Unit) {
        while (true) {
            delay(60_000)
            now = System.currentTimeMillis() / 1000
        }
    }

    LaunchedEffect(style, friends, now) {
        val s = style ?: return@LaunchedEffect
        updateFriends(density, s, icons, friends, now)
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
        val m = map ?: return@LaunchedEffect
        if (f.friendId == null) {
            fitAll(m, friends, animate = true)
            return@LaunchedEffect
        }
        val loc = friends.firstOrNull { it.accountId == f.friendId }?.location ?: return@LaunchedEffect
        if (loc.kind == LocationKind.HIDDEN) return@LaunchedEffect
        m.animateCamera(CameraUpdateFactory.newLatLngZoom(LatLng(loc.latE7 / 1e7, loc.lonE7 / 1e7), 15.0))
    }

    AndroidView(factory = { mapView }, modifier = modifier)
}

private const val SOURCE_FRIENDS = "staya-friends"
private const val SOURCE_ACCURACY = "staya-accuracy"
private const val ACCENT = "#4CD97B"

private fun addLayers(style: Style) {
    style.addSource(GeoJsonSource(SOURCE_ACCURACY))
    style.addSource(GeoJsonSource(SOURCE_FRIENDS))
    style.addLayer(
        FillLayer("staya-accuracy-fill", SOURCE_ACCURACY).withProperties(
            PropertyFactory.fillColor(ACCENT),
            PropertyFactory.fillOpacity(0.12f),
        ),
    )
    style.addLayer(
        LineLayer("staya-accuracy-line", SOURCE_ACCURACY).withProperties(
            PropertyFactory.lineColor(ACCENT),
            PropertyFactory.lineOpacity(0.5f),
            PropertyFactory.lineWidth(1f),
        ),
    )
    style.addLayer(
        SymbolLayer("staya-friends", SOURCE_FRIENDS).withProperties(
            PropertyFactory.iconImage(Expression.get("icon")),
            PropertyFactory.iconAllowOverlap(true),
            PropertyFactory.iconIgnorePlacement(true),
            // Точка — центр аватара, а не центр картинки с бейджем (см. markerBitmap).
            PropertyFactory.iconOffset(arrayOf(0f, MARKER_SHIFT_DP)),
        ),
    )
}

private fun updateFriends(density: Float, style: Style, icons: MutableMap<String, String>, friends: List<FriendView>, now: Long) {
    val points = mutableListOf<Feature>()
    val circles = mutableListOf<Feature>()
    val seen = mutableSetOf<String>()
    for (f in friends) {
        val loc = f.location ?: continue
        if (loc.kind == LocationKind.HIDDEN) continue
        val badge = MapMath.shortAge(now, loc.timestamp)
        val fresh = now - loc.timestamp < 10 * 60
        // Имя картинки меняется вместе с аватаром, ником и бейджем — новая картинка, а не старая из кэша стиля.
        val icon = "avatar-" + f.accountId + "-" + (f.avatar?.contentHashCode() ?: 0) + "-" + f.nick.hashCode() + "-" + badge + fresh
        val previous = icons.put(f.accountId, icon)
        if (previous != null && previous != icon) style.removeImage(previous)
        if (style.getImage(icon) == null) style.addImage(icon, markerBitmap(f, badge, fresh, density))
        seen += f.accountId
        val point = Point.fromLngLat(loc.lonE7 / 1e7, loc.latE7 / 1e7)
        points += Feature.fromGeometry(point).apply { addStringProperty("icon", icon) }
        if (loc.accuracyM.toInt() > 0) {
            val ring = MapMath.accuracyRing(loc.latE7, loc.lonE7, loc.accuracyM.toDouble())
                .map { Point.fromLngLat(it[0], it[1]) }
            circles += Feature.fromGeometry(Polygon.fromLngLats(listOf(ring)))
        }
    }
    (icons.keys - seen).forEach { id -> icons.remove(id)?.let(style::removeImage) }
    style.getSourceAs<GeoJsonSource>(SOURCE_FRIENDS)?.setGeoJson(FeatureCollection.fromFeatures(points))
    style.getSourceAs<GeoJsonSource>(SOURCE_ACCURACY)?.setGeoJson(FeatureCollection.fromFeatures(circles))
}

/** Подпись в списке: где друг и насколько свежо. */
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

private const val AVATAR_DP = 48
private const val RING_DP = 3
private const val BADGE_H_DP = 20
private const val OVERLAP_DP = 6
/** Картинка выше аватара на высоту бейджа минус перекрытие: смещаем на половину вниз. */
private const val MARKER_SHIFT_DP = (BADGE_H_DP - OVERLAP_DP) / 2f

/**
 * Аватар в белом кольце (без аватара — первая буква ника на мятном), под ним бейдж
 * давности: тёмная пилюля с белым текстом и зелёной точкой, если позиция свежая.
 */
private fun markerBitmap(f: FriendView, badge: String, fresh: Boolean, density: Float): Bitmap {
    val avatar = AVATAR_DP * density
    val ring = RING_DP * density
    val badgeH = BADGE_H_DP * density
    val overlap = OVERLAP_DP * density
    val text = Paint(Paint.ANTI_ALIAS_FLAG).apply {
        color = 0xFFFFFFFF.toInt()
        textSize = 11.5f * density
        typeface = Typeface.create(Typeface.DEFAULT, Typeface.BOLD)
        textAlign = Paint.Align.CENTER
    }
    val dot = if (fresh) 6 * density else 0f
    val badgeW = text.measureText(badge) + 16 * density + (if (fresh) dot + 4 * density else 0f)
    val width = maxOf(avatar, badgeW) + 4 * density
    val height = avatar + badgeH - overlap + 4 * density
    val out = createBitmap(width.toInt(), height.toInt())
    val canvas = Canvas(out)
    val paint = Paint(Paint.ANTI_ALIAS_FLAG)
    val cx = width / 2f
    val top = 2 * density

    // Кольцо и аватар.
    paint.color = 0xFFFFFFFF.toInt()
    paint.setShadowLayer(4 * density, 0f, 2 * density, 0x55000000)
    canvas.drawCircle(cx, top + avatar / 2, avatar / 2, paint)
    paint.clearShadowLayer()
    val inner = RectF(cx - avatar / 2 + ring, top + ring, cx + avatar / 2 - ring, top + avatar - ring)
    val picture = f.avatar?.takeIf { it.isNotEmpty() }?.let { BitmapFactory.decodeByteArray(it, 0, it.size) }
    if (picture != null) {
        val layer = canvas.saveLayer(inner, null)
        paint.color = 0xFF000000.toInt()
        canvas.drawOval(inner, paint)
        paint.xfermode = PorterDuffXfermode(PorterDuff.Mode.SRC_IN)
        canvas.drawBitmap(picture, Rect(0, 0, picture.width, picture.height), inner, paint)
        paint.xfermode = null
        canvas.restoreToCount(layer)
    } else {
        paint.color = 0xFFA9F0C7.toInt()
        canvas.drawOval(inner, paint)
        paint.color = 0xFF0F0F12.toInt()
        paint.textSize = avatar * 0.42f
        paint.typeface = Typeface.create(Typeface.DEFAULT, Typeface.BOLD)
        paint.textAlign = Paint.Align.CENTER
        val letter = f.nick?.trim()?.firstOrNull()?.uppercase() ?: "?"
        val y = inner.centerY() - (paint.descent() + paint.ascent()) / 2
        canvas.drawText(letter, cx, y, paint)
    }

    // Бейдж.
    val badgeTop = top + avatar - overlap
    val pill = RectF(cx - badgeW / 2, badgeTop, cx + badgeW / 2, badgeTop + badgeH)
    paint.color = 0xFF0F0F12.toInt()
    canvas.drawRoundRect(pill, badgeH / 2, badgeH / 2, paint)
    paint.color = 0xFFFFFFFF.toInt()
    paint.style = Paint.Style.STROKE
    paint.strokeWidth = 1.5f * density
    canvas.drawRoundRect(pill, badgeH / 2, badgeH / 2, paint)
    paint.style = Paint.Style.FILL
    var textX = cx
    if (fresh) {
        paint.color = 0xFF4CD97B.toInt()
        canvas.drawCircle(pill.left + 8 * density + dot / 2, pill.centerY(), dot / 2, paint)
        textX += (dot + 4 * density) / 2
    }
    val ty = pill.centerY() - (text.descent() + text.ascent()) / 2
    canvas.drawText(badge, textX, ty, text)
    return out
}

private val boundsWorker = Executors.newSingleThreadExecutor()

/** Все видимые друзья в кадре; одного — крупно. `false` — показывать было некого. */
private fun fitAll(map: MapLibreMap, friends: List<FriendView>, animate: Boolean): Boolean {
    val visible = friends.mapNotNull { it.location }.filter { it.kind != LocationKind.HIDDEN }
    val update = when {
        visible.size == 1 -> CameraUpdateFactory.newLatLngZoom(LatLng(visible[0].latE7 / 1e7, visible[0].lonE7 / 1e7), 14.0)
        visible.size > 1 -> {
            val bounds = LatLngBounds.Builder()
            visible.forEach { bounds.include(LatLng(it.latE7 / 1e7, it.lonE7 / 1e7)) }
            CameraUpdateFactory.newLatLngBounds(bounds.build(), 160)
        }
        else -> return false
    }
    if (animate) map.animateCamera(update) else map.moveCamera(update)
    return true
}

/** Первый показ: все друзья на карте; если их нет — регион карты сервера. */
private fun fitInitial(map: MapLibreMap, host: String, friends: List<FriendView>, stillRegion: () -> Boolean) {
    if (fitAll(map, friends, animate = false)) return
    boundsWorker.execute {
        val b = MapNetwork.regionBounds(host) ?: return@execute
        val region = LatLngBounds.from(b[3], b[2], b[1], b[0])
        android.os.Handler(android.os.Looper.getMainLooper()).post {
            if (stillRegion()) map.moveCamera(CameraUpdateFactory.newLatLngBounds(region, 0))
        }
    }
}
