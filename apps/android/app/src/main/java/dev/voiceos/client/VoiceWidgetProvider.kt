package dev.voiceos.client

import android.app.PendingIntent
import android.appwidget.AppWidgetManager
import android.appwidget.AppWidgetProvider
import android.content.ComponentName
import android.content.Context
import android.content.Intent
import android.net.Uri
import android.view.View
import android.widget.RemoteViews

class VoiceWidgetProvider : AppWidgetProvider() {
    override fun onUpdate(
        context: Context,
        appWidgetManager: AppWidgetManager,
        appWidgetIds: IntArray,
    ) {
        appWidgetIds.forEach { appWidgetId ->
            appWidgetManager.updateAppWidget(appWidgetId, views(context, savedStatus(context)))
        }
        refreshTasks(context)
    }

    override fun onEnabled(context: Context) {
        super.onEnabled(context)
        refreshTasks(context)
    }

    override fun onReceive(context: Context, intent: Intent) {
        super.onReceive(context, intent)
        when (intent.action) {
            ACTION_REFRESH_TASKS -> refreshTasks(context)
            ACTION_COMPLETE_TASK -> completeTask(context, intent.data?.lastPathSegment)
        }
    }

    companion object {
        fun updateStatus(context: Context, status: String) {
            context.getSharedPreferences(PREFERENCES, Context.MODE_PRIVATE)
                .edit()
                .putString(STATUS, status)
                .apply()
            updateAll(context)
        }

        fun refreshTasks(context: Context, onComplete: ((Result<List<VoiceTask>>) -> Unit)? = null) {
            GatewayClient.getTasks(
                GatewaySettings.baseUrl(context),
                DeviceCredentials.token(context),
            ) { result ->
                result.onSuccess { TaskWidgetStore.save(context, it) }
                updateAll(context)
                onComplete?.invoke(result)
            }
        }

        private fun completeTask(context: Context, taskId: String?) {
            if (taskId.isNullOrBlank()) return
            val original = TaskWidgetStore.load(context)
            TaskWidgetStore.save(context, original.filterNot { it.id == taskId })
            updateAll(context)
            GatewayClient.updateTaskStatus(
                GatewaySettings.baseUrl(context),
                taskId,
                "completed",
                DeviceCredentials.token(context),
            ) { result ->
                if (result.isFailure) {
                    TaskWidgetStore.save(context, original)
                    updateAll(context)
                } else {
                    refreshTasks(context)
                }
            }
        }

        private fun updateAll(context: Context) {
            val manager = AppWidgetManager.getInstance(context)
            val component = ComponentName(context, VoiceWidgetProvider::class.java)
            manager.updateAppWidget(component, views(context, savedStatus(context)))
        }

        private fun savedStatus(context: Context): String = context
            .getSharedPreferences(PREFERENCES, Context.MODE_PRIVATE)
            .getString(STATUS, null)
            ?.takeIf { it.isNotBlank() }
            ?: "Ready"

        private fun views(context: Context, status: String): RemoteViews {
            val tasks = TaskWidgetStore.load(context)
                .filter { it.status !in setOf("completed", "cancelled") }
                .sortedBy {
                    when (it.progressLane) {
                        "needs_me" -> 0
                        "review" -> 1
                        "vic_working" -> 2
                        else -> 3
                    }
                }
            return RemoteViews(context.packageName, R.layout.voice_widget).apply {
                setTextViewText(R.id.widget_status, "VoiceOS • $status")
                setTextViewText(R.id.widget_gateway, GatewaySettings.displayName(context))
                setTextViewText(
                    R.id.widget_task_summary,
                    if (tasks.isEmpty()) "TASK PROGRESS • CLEAR" else {
                        val needsMe = tasks.count { it.progressLane == "needs_me" }
                        val vic = tasks.count { it.progressLane == "vic_working" }
                        val review = tasks.count { it.progressLane == "review" }
                        "NEEDS ME $needsMe • VIC $vic • REVIEW $review"
                    },
                )
                setOnClickPendingIntent(R.id.widget_talk, activityIntent(context, MainActivity.ACTION_WIDGET_TALK, 101))
                setOnClickPendingIntent(R.id.widget_add_task, activityIntent(context, MainActivity.ACTION_WIDGET_ADD_TASK, 102))
                setOnClickPendingIntent(R.id.widget_refresh, refreshIntent(context))
                renderTaskRows(context, this, tasks.take(3))
            }
        }

        private fun renderTaskRows(context: Context, views: RemoteViews, tasks: List<VoiceTask>) {
            val rows = intArrayOf(R.id.widget_task_row_1, R.id.widget_task_row_2, R.id.widget_task_row_3)
            val titles = intArrayOf(R.id.widget_task_title_1, R.id.widget_task_title_2, R.id.widget_task_title_3)
            val metadata = intArrayOf(R.id.widget_task_meta_1, R.id.widget_task_meta_2, R.id.widget_task_meta_3)
            rows.indices.forEach { index ->
                val task = tasks.getOrNull(index)
                views.setViewVisibility(rows[index], if (task == null) View.GONE else View.VISIBLE)
                if (task != null) {
                    val marker = when (task.progressLane) {
                        "needs_me" -> "ME"
                        "vic_working" -> "VIC"
                        "review" -> "REVIEW"
                        else -> "SHARED"
                    }
                    views.setTextViewText(titles[index], "$marker  ${task.title}")
                    views.setTextViewText(
                        metadata[index],
                        when (task.progressLane) {
                            "needs_me" -> task.nextUserAction.ifBlank { "Your action is needed" }
                            "vic_working" -> task.nextVicAction.ifBlank { "VIC is working" }
                            "review" -> task.nextUserAction.ifBlank { "Ready for your review" }
                            else -> "${task.completedSteps}/${task.totalSteps} steps • ${task.openBlockers} blockers"
                        },
                    )
                    views.setOnClickPendingIntent(rows[index], completeIntent(context, task.id))
                }
            }
            views.setViewVisibility(R.id.widget_empty_tasks, if (tasks.isEmpty()) View.VISIBLE else View.GONE)
        }

        private fun activityIntent(context: Context, action: String, requestCode: Int): PendingIntent {
            val intent = Intent(context, MainActivity::class.java).apply {
                this.action = action
                flags = Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_CLEAR_TOP or Intent.FLAG_ACTIVITY_SINGLE_TOP
            }
            return PendingIntent.getActivity(
                context,
                requestCode,
                intent,
                PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
            )
        }

        private fun refreshIntent(context: Context): PendingIntent {
            val intent = Intent(context, VoiceWidgetProvider::class.java).apply {
                action = ACTION_REFRESH_TASKS
            }
            return PendingIntent.getBroadcast(
                context,
                201,
                intent,
                PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
            )
        }

        private fun completeIntent(context: Context, taskId: String): PendingIntent {
            val intent = Intent(context, VoiceWidgetProvider::class.java).apply {
                action = ACTION_COMPLETE_TASK
                data = Uri.parse("voiceos://task/$taskId")
            }
            return PendingIntent.getBroadcast(
                context,
                taskId.hashCode(),
                intent,
                PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
            )
        }

        private const val PREFERENCES = "voiceos_widget"
        private const val STATUS = "status"
        private const val ACTION_REFRESH_TASKS = "dev.voiceos.client.action.REFRESH_TASKS"
        private const val ACTION_COMPLETE_TASK = "dev.voiceos.client.action.COMPLETE_TASK"
    }
}
