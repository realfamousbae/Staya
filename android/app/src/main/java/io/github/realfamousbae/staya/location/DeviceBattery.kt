package io.github.realfamousbae.staya.location

import android.content.Context
import android.os.BatteryManager
import uniffi.staya_core.StayaCore

/** Read only after consent; never log or add battery updates to the control queue. */
object DeviceBattery {
    fun refresh(context: Context, core: StayaCore) {
        if (!core.batterySharing()) {
            core.setDeviceBattery(null, false)
            return
        }
        val manager = context.getSystemService(BatteryManager::class.java)
        val percent = manager.getIntProperty(BatteryManager.BATTERY_PROPERTY_CAPACITY)
        core.setDeviceBattery(percent.takeIf { it in 0..100 }?.toUByte(), manager.isCharging)
    }
}
