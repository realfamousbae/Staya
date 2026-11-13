package io.github.realfamousbae.staya.places

import android.Manifest
import android.app.Application
import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Intent
import android.content.pm.PackageManager
import android.os.Build
import io.github.realfamousbae.staya.MainActivity
import io.github.realfamousbae.staya.R
import uniffi.staya_core.CoreEvent
import uniffi.staya_core.StayaCore

object PlaceNotifications {
    private var app: Application? = null
    fun initialize(application: Application) { app = application }
    fun cancel(place: String) { app?.getSystemService(NotificationManager::class.java)?.cancel(place, 1) }
    fun cancelFriend(friend: String) {
        val manager = app?.getSystemService(NotificationManager::class.java) ?: return
        manager.activeNotifications.filter { it.notification.extras.getString("staya-friend") == friend }
            .forEach { manager.cancel(it.tag, it.id) }
    }
    fun post(core: StayaCore, events: List<CoreEvent>) {
        val context = app ?: return
        val manager = context.getSystemService(NotificationManager::class.java)
        for (removed in events.filterIsInstance<CoreEvent.FriendRemoved>()) {
            cancelFriend(removed.friend)
        }
        if (Build.VERSION.SDK_INT >= 33 && context.checkSelfPermission(Manifest.permission.POST_NOTIFICATIONS) != PackageManager.PERMISSION_GRANTED) return
        if (!manager.areNotificationsEnabled()) return
        val transitions = events.filterIsInstance<CoreEvent.PlaceTransition>()
        if (transitions.isEmpty()) return
        manager.createNotificationChannel(NotificationChannel("places", "Места", NotificationManager.IMPORTANCE_DEFAULT))
        val protected = context.getSharedPreferences("app-lock", 0).getBoolean("enabled", false)
        val friends = runCatching { core.listFriends() }.getOrDefault(emptyList())
        for (event in transitions) {
            val friend = friends.find { it.accountId == event.friend }?.nick ?: "Друг"
            val body = if (protected) "Изменилось присутствие друга. Открой Staya."
                else "$friend ${if (event.entered) "пришёл" else "ушёл"}: ${event.name}"
            val intent = PendingIntent.getActivity(context, 0, Intent(context, MainActivity::class.java), PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT)
            val public = Notification.Builder(context, "places").setSmallIcon(R.drawable.ic_launcher_foreground).setContentTitle("Staya").setContentText("Новое уведомление").build()
            val notification = Notification.Builder(context, "places").setSmallIcon(R.drawable.ic_launcher_foreground)
                .setContentTitle("Staya — места").setContentText(body).setContentIntent(intent).setAutoCancel(true)
                .addExtras(android.os.Bundle().apply { putString("staya-friend", event.friend) })
                .setVisibility(Notification.VISIBILITY_PRIVATE).setPublicVersion(public).build()
            // Keep only the latest transition per place; no notification history in the app.
            runCatching { manager.notify(event.place, 1, notification) }
        }
    }
}
