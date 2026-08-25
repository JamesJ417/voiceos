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
                        .put("project_id", task.projectId)
                        .put("parent_task_id", task.parentTaskId)
                        .put("title", task.title)
                        .put("observable_outcome", task.observableOutcome)
                        .put("estimated_minutes", task.estimatedMinutes)
                        .put("due_at", task.dueAt)
                        .put("importance", task.importance)
                        .put("status", task.status)
                        .put("updated_at", task.updatedAt)
                        .put("progress_lane", task.progressLane)
                        .put("vic_status", task.vicStatus)
                        .put("completed_steps", task.completedSteps)
                        .put("total_steps", task.totalSteps)
                        .put("open_blockers", task.openBlockers)
                        .put("next_user_action", task.nextUserAction)
                        .put("next_vic_action", task.nextVicAction),
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
                            projectId = task.optString("project_id").takeIf { it.isNotBlank() && it != "null" },
                            parentTaskId = task.optString("parent_task_id").takeIf { it.isNotBlank() && it != "null" },
                            title = task.getString("title"),
                            observableOutcome = task.optString("observable_outcome"),
                            estimatedMinutes = task.optInt("estimated_minutes", 20),
                            dueAt = task.optString("due_at").takeIf { it.isNotBlank() && it != "null" },
                            importance = task.optString("importance", "normal"),
                            status = task.optString("status", "ready"),
                            updatedAt = task.optString("updated_at"),
                            progressLane = task.optString("progress_lane", "shared"),
                            vicStatus = task.optString("vic_status", "not_analyzed"),
                            completedSteps = task.optInt("completed_steps"),
                            totalSteps = task.optInt("total_steps"),
                            openBlockers = task.optInt("open_blockers"),
                            nextUserAction = task.optString("next_user_action"),
                            nextVicAction = task.optString("next_vic_action"),
                        ),
                    )
                }
            }
        }.getOrDefault(emptyList())
    }

    fun replace(context: Context, updated: VoiceTask) {
        val current = load(context)
        val found = current.any { it.id == updated.id }
        save(
            context,
            if (found) current.map { if (it.id == updated.id) updated else it }
            else listOf(updated) + current,
        )
    }

    fun lastSyncMillis(context: Context): Long =
        context.getSharedPreferences(PREFERENCES, Context.MODE_PRIVATE)
            .getLong(LAST_SYNC, 0L)
}
