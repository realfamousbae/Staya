package io.github.realfamousbae.staya

import android.app.Application

class StayaApplication : Application() {
    override fun onCreate() {
        super.onCreate()
        LegacyCleanup.run(this)
    }
}
