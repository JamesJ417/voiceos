package dev.voiceos.client

/** UI projection for the Messages tab, kept independent of Android views. */
object BackgroundMessageInboxModel {
    fun unreadCount(messages: List<BackgroundMessage>): Int = messages.count { !it.read }
    fun markRead(messages: List<BackgroundMessage>, id: String): List<BackgroundMessage> =
        messages.map { if (it.id == id) it.copy(read = true) else it }
}
