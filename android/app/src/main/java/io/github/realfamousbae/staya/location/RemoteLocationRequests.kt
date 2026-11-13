package io.github.realfamousbae.staya.location

import android.app.Application
import uniffi.staya_core.CoreEvent

/** The core has already checked consent, privacy modes, sender and rate limits. */
object RemoteLocationRequests {
    private var app: Application? = null
    fun initialize(application: Application) { app = application }
    fun handle(events: List<CoreEvent>) {
        if (events.none { it is CoreEvent.LocationRequested }) return
        val context = app ?: return
        LocationShare.startService(context, fromBackground = true, tick = true)
    }
}
