package io.github.realfamousbae.staya.map

import android.Manifest
import android.annotation.SuppressLint
import android.location.LocationListener
import android.location.LocationManager
import android.os.Looper
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.runtime.*
import androidx.compose.ui.platform.LocalContext
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.LifecycleEventObserver
import androidx.lifecycle.compose.LocalLifecycleOwner
import io.github.realfamousbae.staya.location.Fix
import io.github.realfamousbae.staya.location.LocationPolicy
import io.github.realfamousbae.staya.location.LocationShare

/** Foreground-only, memory-only map position. Never enters the sharing queue. */
@SuppressLint("MissingPermission")
@Composable
fun rememberOwnPosition(): Fix? {
    val context = LocalContext.current
    val lifecycle = LocalLifecycleOwner.current.lifecycle
    var position by remember { mutableStateOf<Fix?>(null) }
    var permitted by remember { mutableStateOf(LocationShare.hasForegroundPermission(context)) }
    val ask = rememberLauncherForActivityResult(ActivityResultContracts.RequestMultiplePermissions()) {
        permitted = LocationShare.hasForegroundPermission(context)
    }
    LaunchedEffect(Unit) {
        if (!permitted) ask.launch(arrayOf(Manifest.permission.ACCESS_FINE_LOCATION, Manifest.permission.ACCESS_COARSE_LOCATION))
    }
    DisposableEffect(lifecycle, permitted) {
        val manager = context.getSystemService(LocationManager::class.java)
        val listener = LocationListener { loc ->
            val fix = Fix(loc.latitude, loc.longitude, if (loc.hasAccuracy()) loc.accuracy else null, loc.time)
            if (LocationPolicy.toCore(fix) != null && LocationPolicy.isFresh(fix, System.currentTimeMillis())) position = fix
        }
        fun stop() { manager.removeUpdates(listener) }
        fun start() {
            stop()
            if (LocationShare.hasForegroundPermission(context)) {
                for (provider in listOf(LocationManager.GPS_PROVIDER, LocationManager.NETWORK_PROVIDER)) {
                    runCatching {
                        if (manager.isProviderEnabled(provider)) manager.requestLocationUpdates(provider, 10_000L, 10f, listener, Looper.getMainLooper())
                    }
                }
            }
        }
        val observer = LifecycleEventObserver { _, event ->
            if (event == Lifecycle.Event.ON_RESUME) start()
            if (event == Lifecycle.Event.ON_PAUSE) stop()
        }
        lifecycle.addObserver(observer)
        if (lifecycle.currentState.isAtLeast(Lifecycle.State.RESUMED)) start()
        onDispose { lifecycle.removeObserver(observer); stop() }
    }
    return position
}
