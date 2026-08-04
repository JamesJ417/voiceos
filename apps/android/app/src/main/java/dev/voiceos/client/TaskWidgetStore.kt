package dev.voiceos.client

import android.content.Context
import org.json.JSONArray
import org.json.JSONObject

object TaskWidgetStore {
    private const val PREFERENCES = "voiceos_widget_tasks"
    private const val TASKS = "tasks"
    private const val LAST_SYNC = "last_sync"

    fun save(context: Context, tasks: List<VoiceTask>) {
        val encoded = JSONArray().apply {
            tasks.take(50).forEach { task ->
                put(
                    JSONObject()
                        .put("id", task.id)
                        .put("title", task.title)
                        .put("observable_outcome", task.observableOutcome)
                        .put("estimated_minutes", task.estimatedMinutes)
                        .put("status", task.status)
                        .put("updated_at", task.updatedAt),
                )
            }
        }
        context.getSharedPreferences(PREFERENCES, Context.MODE_PRIVATE)
            .edit()
            .putString(TASKS, encoded.toString())
            .putLong(LAST_SYNC, System.currentTimeMillis())
            .apply()
    }

    fun load(context: Context): List<VoiceTask> {
        val raw = context.getSharedPreferences(PREFERENCES, Context.MODE_PRIVATE)
            .getString(TASKS, null)
            ?: return emptyList()
        return runCatching {
            val array = JSONArray(raw)
            buildList {
                for (index in 0 until array.length()) {
                    val task = array.getJSONObject(index)
                    add(
                        VoiceTask(
                            id = task.getString("id"),
                            title = task.getString("title"),
                            observableOutcome = task.optString("observable_outcome"),
                            estimatedMinutes = task.optInt("estimated_minutes", 20),
                            status = task.optString("status", "ready"),
                            updatedAt = task.optString("updated_at"),
                        ),
                    )
                }
            }
        }.getOrDefault(emptyList())
    }

    fun lastSyncMillis(context: Context): Long =
        context.getSharedPreferences(PREFERENCES, Context.MODE_PRIVATE)
            .getLong(LAST_SYNC, 0L)
}
