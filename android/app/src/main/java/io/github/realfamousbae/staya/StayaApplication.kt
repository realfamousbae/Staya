package io.github.realfamousbae.staya

import android.app.Application
import io.github.realfamousbae.staya.probe.Probe

class StayaApplication : Application() {
    override fun onCreate() {
        super.onCreate()
        Probe.init(this)
    }
}
