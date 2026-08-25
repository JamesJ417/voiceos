package dev.voiceos.client

import android.app.PendingIntent
import android.appwidget.AppWidgetManager
import android.appwidget.AppWidgetProvider
import android.content.ComponentName
import android.content.Context
import android.content.Intent
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
            ACTION_START_TASK -> startTask(context, intent.getStringExtra(EXTRA_TASK_ID))
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
            val selection = FocusWidgetModel.select(TaskWidgetStore.load(context))
            val task = selection.primary
            val rows = listOf(
                Triple(R.id.widget_task_row_1, R.id.widget_task_title_1, R.id.widget_task_meta_1),
                Triple(R.id.widget_task_row_2, R.id.widget_task_title_2, R.id.widget_task_meta_2),
                Triple(R.id.widget_task_row_3, R.id.widget_task_title_3, R.id.widget_task_meta_3),
            )
            return RemoteViews(context.packageName, R.layout.voice_widget).apply {
                setTextViewText(R.id.widget_status, "OV • $status")
                setTextViewText(
                    R.id.widget_gateway,
                    if (selection.parkedCount == 0) GatewaySettings.displayName(context)
                    else "${selection.parkedCount} safely parked • ${GatewaySettings.displayName(context)}",
                )
                setTextViewText(
                    R.id.widget_task_summary,
                    when {
                        task == null -> "TODAY • CLEAR"
                        task.status == "active" -> "★ STAY WITH #1 • OR CHOOSE ANOTHER"
                        else -> "★ VIC RECOMMENDS #1 • TAP TO CHOOSE"
                    },
                )
                setOnClickPendingIntent(R.id.widget_talk, activityIntent(context, MainActivity.ACTION_WIDGET_TALK, 101))
                setOnClickPendingIntent(R.id.widget_add_task, activityIntent(context, MainActivity.ACTION_WIDGET_ADD_TASK, 102))
                setOnClickPendingIntent(R.id.widget_refresh, refreshIntent(context))
                setViewVisibility(R.id.widget_empty_tasks, if (task == null) View.VISIBLE else View.GONE)
                setViewVisibility(R.id.widget_start_task, if (task == null) View.GONE else View.VISIBLE)
                rows.forEachIndexed { index, (rowId, titleId, metaId) ->
                    val choice = selection.choices.getOrNull(index)
                    setViewVisibility(rowId, if (choice == null) View.GONE else View.VISIBLE)
                    if (choice != null) {
                        setTextViewText(titleId, "${index + 1}. ${choice.title}")
                        setTextViewText(
                            metaId,
                            if (index == 0) {
                                "★ ${FocusWidgetModel.recommendationReason(choice)} • ${FocusWidgetModel.nextAction(choice)}"
                            } else {
                                "${choice.estimatedMinutes.coerceAtLeast(1)} MIN • ${FocusWidgetModel.nextAction(choice)}"
                            },
                        )
                        setContentDescription(
                            rowId,
                            if (index == 0) "VIC recommends ${choice.title}. Tap to choose it."
                            else "Option ${index + 1}: ${choice.title}. Tap to choose it.",
                        )
                        setOnClickPendingIntent(rowId, startTaskIntent(context, choice.id))
                    }
                }
                if (task != null) {
                    setOnClickPendingIntent(R.id.widget_start_task, startTaskIntent(context, task.id))
                    setTextViewText(
                        R.id.widget_start_task,
                        if (task.status == "active") "KEEP GOING" else "START VIC PICK",
                    )
                }
            }
        }

        private fun startTask(context: Context, taskId: String?) {
            if (taskId.isNullOrBlank()) return
            updateStatus(context, "Starting focus")
            GatewayClient.updateTaskStatus(
                GatewaySettings.baseUrl(context),
                taskId,
                "active",
                DeviceCredentials.token(context),
            ) { result ->
                result.onSuccess { TaskWidgetStore.replace(context, it) }
                updateStatus(context, if (result.isSuccess) "Focus started" else "Tap to reconnect")
            }
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

        private fun startTaskIntent(context: Context, taskId: String): PendingIntent {
            val intent = Intent(context, VoiceWidgetProvider::class.java).apply {
                action = ACTION_START_TASK
                putExtra(EXTRA_TASK_ID, taskId)
            }
            return PendingIntent.getBroadcast(
                context,
                taskId.hashCode() xor 0x51A7,
                intent,
                PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
            )
        }

        private fun openTaskIntent(context: Context, taskId: String): PendingIntent {
            val intent = Intent(context, MainActivity::class.java).apply {
                action = MainActivity.ACTION_WIDGET_OPEN_FEED
                flags = Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_CLEAR_TOP or Intent.FLAG_ACTIVITY_SINGLE_TOP
                putExtra(MainActivity.EXTRA_TASK_ID, taskId)
            }
            return PendingIntent.getActivity(
                context,
                taskId.hashCode(),
                intent,
                PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
            )
        }

        private const val PREFERENCES = "voiceos_widget"
        private const val STATUS = "status"
        private const val ACTION_REFRESH_TASKS = "dev.voiceos.client.action.REFRESH_TASKS"
        private const val ACTION_START_TASK = "dev.voiceos.client.action.START_FOCUS_TASK"
        private const val EXTRA_TASK_ID = "task_id"
    }
}
