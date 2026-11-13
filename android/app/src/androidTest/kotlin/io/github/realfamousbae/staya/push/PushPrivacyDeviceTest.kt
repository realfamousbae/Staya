package io.github.realfamousbae.staya.push

import android.content.pm.PackageManager
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import com.google.firebase.messaging.FirebaseMessaging
import io.github.realfamousbae.staya.AppCore
import io.github.realfamousbae.staya.BuildConfig
import org.junit.Assert.*
import org.junit.Assume.assumeFalse
import org.junit.Test
import org.junit.runner.RunWith

@RunWith(AndroidJUnit4::class)
class PushPrivacyDeviceTest {
    @Test fun sdkDefaultsDoNotEnableCollectionOrAdvertising() {
        val context = InstrumentationRegistry.getInstrumentation().targetContext
        val metadata = context.packageManager.getApplicationInfo(context.packageName, PackageManager.GET_META_DATA).metaData
        for (key in listOf("firebase_messaging_auto_init_enabled", "firebase_analytics_collection_enabled", "firebase_messaging_notification_delegation_enabled", "delivery_metrics_exported_to_big_query_enabled")) {
            assertFalse(key, metadata.getBoolean(key, true))
        }
        if (BuildConfig.FCM_CONFIGURED) assertFalse(FirebaseMessaging.getInstance().isAutoInitEnabled)
        for (name in listOf("com.google.firebase.analytics.FirebaseAnalytics", "com.google.firebase.crashlytics.FirebaseCrashlytics")) {
            assertTrue("unexpected collection SDK: $name", runCatching { Class.forName(name, false, context.classLoader) }.isFailure)
        }
        val permissions = context.packageManager.getPackageInfo(context.packageName, PackageManager.GET_PERMISSIONS).requestedPermissions.orEmpty()
        assertFalse(permissions.contains("com.google.android.gms.permission.AD_ID"))
    }
    @Test fun disabledPushDoesNotOpenTheCoreOrStartNetworkWork() {
        val context = InstrumentationRegistry.getInstrumentation().targetContext
        assumeFalse(FcmPush.isEnabled(context))
        assumeFalse(context.getSharedPreferences("fcm", 0).getBoolean("cleanup", false))
        val before = AppCore.state
        assertTrue(FcmPush.synchronize(context))
        assertSame(before, AppCore.state)
    }
}
