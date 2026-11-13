package io.github.realfamousbae.staya.location

import android.app.Application

/** Native UI consent survives a crash before the queued core mutation runs. */
object NativeSharingConsent {
    @Volatile private var app: Application? = null
    fun initialize(application: Application) { app = application }
    fun allowed(): Boolean = app?.let { LocationShare.isEnabled(it) } ?: false
}
