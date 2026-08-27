package dev.voiceos.client

import android.content.Context
import org.json.JSONArray
import org.json.JSONObject

data class BackgroundMessage(val id: String, val text: String, val read: Boolean = false)

/** Durable inbox for background VIC messages; never feeds the active transcript. */
class BackgroundMessageStore(private val context: Context) {
    private val prefs get() = context.getSharedPreferences("background_messages", Context.MODE_PRIVATE)
    fun messages(): List<BackgroundMessage> = runCatching {
        val json = JSONArray(prefs.getString(KEY, "[]"))
        (0 until json.length()).map { val o = json.getJSONObject(it); BackgroundMessage(o.getString("id"), o.getString("text"), o.optBoolean("read")) }
    }.getOrDefault(emptyList())
    fun add(id: String, text: String): Boolean {
        if (id.isBlank() || text.isBlank() || messages().any { it.id == id }) return false
        save(messages() + BackgroundMessage(id, text)); return true
    }
    fun markRead(id: String) = save(messages().map { if (it.id == id) it.copy(read = true) else it })
    fun unreadCount() = messages().count { !it.read }
    private fun save(items: List<BackgroundMessage>) { prefs.edit().putString(KEY, JSONArray(items.map { JSONObject().put("id", it.id).put("text", it.text).put("read", it.read) }).toString()).apply() }
    companion object { private const val KEY = "inbox" }
}


