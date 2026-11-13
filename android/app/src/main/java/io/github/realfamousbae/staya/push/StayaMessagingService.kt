package io.github.realfamousbae.staya.push

import com.google.firebase.messaging.FirebaseMessagingService
import com.google.firebase.messaging.RemoteMessage

class StayaMessagingService : FirebaseMessagingService() {
    override fun onNewToken(token: String) {
        // The worker retrieves the current SDK token; never log or copy it into work data.
        if (FcmPush.isEnabled(this)) FcmPush.enqueue(this)
    }
    override fun onMessageReceived(message: RemoteMessage) {
        if (FcmPush.isEnabled(this) && message.data == mapOf("wake" to "1")) FcmPush.enqueue(this)
    }
    override fun onDeletedMessages() {
        if (FcmPush.isEnabled(this)) FcmPush.enqueue(this)
    }
}
