package dev.voiceos.client

import android.content.Context
import org.json.JSONArray
import org.json.JSONObject
import java.time.DayOfWeek
import java.time.Instant
import java.time.LocalDate
import java.time.LocalTime
import java.time.ZoneId
import java.time.ZonedDateTime
import java.time.format.DateTimeFormatter
import java.time.format.TextStyle
import java.util.Locale
import java.util.UUID

data class WeeklyTaskDraft(
    val dayOfWeek: Int,
    val hour: Int,
    val minute: Int,
    val timeZone: String = ZoneId.systemDefault().id,
)

data class WeeklyTaskTemplate(
    val id: String,
    val title: String,
    val observableOutcome: String,
    val estimatedMinutes: Int,
    val projectId: String?,
    val dayOfWeek: Int,
    val hour: Int,
    val minute: Int,
    val timeZone: String,
    val activeTaskId: String?,
    val currentDueAt: String?,
    val enabled: Boolean = true,
)

object WeeklyTaskModel {
    fun firstDue(draft: WeeklyTaskDraft, now: ZonedDateTime = ZonedDateTime.now()): String {
        val zone = runCatching { ZoneId.of(draft.timeZone) }.getOrDefault(now.zone)
        val localNow = now.withZoneSameInstant(zone)
        var date = localNow.toLocalDate()
        while (date.dayOfWeek.value != draft.dayOfWeek) date = date.plusDays(1)
        var due = ZonedDateTime.of(date, LocalTime.of(draft.hour, draft.minute), zone)
        if (!due.isAfter(localNow)) due = due.plusWeeks(1)
        return due.toOffsetDateTime().toString()
    }

    fun nextDue(template: WeeklyTaskTemplate, now: ZonedDateTime = ZonedDateTime.now()): String {
        val zone = runCatching { ZoneId.of(template.timeZone) }.getOrDefault(now.zone)
        val localNow = now.withZoneSameInstant(zone)
        val priorDate = template.currentDueAt
            ?.let { runCatching { Instant.parse(it).atZone(zone).toLocalDate() }.getOrNull() }
            ?: template.currentDueAt
                ?.let { runCatching { ZonedDateTime.parse(it).withZoneSameInstant(zone).toLocalDate() }.getOrNull() }
        var date = priorDate?.plusWeeks(1) ?: LocalDate.now(zone)
        while (date.dayOfWeek.value != template.dayOfWeek) date = date.plusDays(1)
        var due = ZonedDateTime.of(date, LocalTime.of(template.hour, template.minute), zone)
        while (!due.isAfter(localNow)) due = due.plusWeeks(1)
        return due.toOffsetDateTime().toString()
    }

    fun scheduleLabel(dayOfWeek: Int, hour: Int, minute: Int): String {
        val day = DayOfWeek.of(dayOfWeek.coerceIn(1, 7))
            .getDisplayName(TextStyle.FULL, Locale.US)
            .uppercase(Locale.US)
        val time = LocalTime.of(hour.coerceIn(0, 23), minute.coerceIn(0, 59))
            .format(DateTimeFormatter.ofPattern("h:mm a", Locale.US))
        return "EVERY $day • DUE $time"
    }
}

object WeeklyTaskStore {
    private const val PREFERENCES = "ov_weekly_tasks"
    private const val TEMPLATES = "templates"

    @Synchronized
    fun load(context: Context): List<WeeklyTaskTemplate> {
        val raw = context.getSharedPreferences(PREFERENCES, Context.MODE_PRIVATE)
            .getString(TEMPLATES, null)
            ?: return emptyList()
        return runCatching {
            val array = JSONArray(raw)
            buildList {
                for (index in 0 until array.length()) {
                    val item = array.getJSONObject(index)
                    add(
                        WeeklyTaskTemplate(
                            id = item.getString("id"),
                            title = item.getString("title"),
                            observableOutcome = item.optString("outcome"),
                            estimatedMinutes = item.optInt("minutes", 20),
                            projectId = item.optString("project_id").takeIf { it.isNotBlank() && it != "null" },
                            dayOfWeek = item.optInt("day", DayOfWeek.MONDAY.value).coerceIn(1, 7),
                            hour = item.optInt("hour", 9).coerceIn(0, 23),
                            minute = item.optInt("minute", 0).coerceIn(0, 59),
                            timeZone = item.optString("time_zone", ZoneId.systemDefault().id),
                            activeTaskId = item.optString("active_task_id").takeIf { it.isNotBlank() && it != "null" },
                            currentDueAt = item.optString("current_due_at").takeIf { it.isNotBlank() && it != "null" },
                            enabled = item.optBoolean("enabled", true),
                        ),
                    )
                }
            }
        }.getOrDefault(emptyList())
    }

    @Synchronized
    fun create(
        context: Context,
        title: String,
        outcome: String,
        minutes: Int,
        projectId: String?,
        draft: WeeklyTaskDraft,
        taskId: String,
        dueAt: String,
    ): WeeklyTaskTemplate {
        val template = WeeklyTaskTemplate(
            id = UUID.randomUUID().toString(),
            title = title,
            observableOutcome = outcome,
            estimatedMinutes = minutes,
            projectId = projectId,
            dayOfWeek = draft.dayOfWeek,
            hour = draft.hour,
            minute = draft.minute,
            timeZone = draft.timeZone,
            activeTaskId = taskId,
            currentDueAt = dueAt,
        )
        save(context, load(context) + template)
        return template
    }

    @Synchronized
    fun updateActive(context: Context, templateId: String, taskId: String, dueAt: String) {
        save(context, load(context).map {
            if (it.id == templateId) it.copy(activeTaskId = taskId, currentDueAt = dueAt) else it
        })
    }

    fun forTask(context: Context, taskId: String): WeeklyTaskTemplate? =
        load(context).firstOrNull { it.enabled && it.activeTaskId == taskId }

    fun labelForTask(context: Context, taskId: String): String? = forTask(context, taskId)?.let {
        WeeklyTaskModel.scheduleLabel(it.dayOfWeek, it.hour, it.minute)
    }

    fun needingNextInstance(context: Context, tasks: List<VoiceTask>): List<WeeklyTaskTemplate> =
        load(context).filter { template ->
            if (!template.enabled) return@filter false
            val activeId = template.activeTaskId ?: return@filter true
            tasks.firstOrNull { it.id == activeId }?.status in setOf("completed", "cancelled")
        }

    @Synchronized
    private fun save(context: Context, templates: List<WeeklyTaskTemplate>) {
        val array = JSONArray()
        templates.forEach { template ->
            array.put(JSONObject().apply {
                put("id", template.id)
                put("title", template.title)
                put("outcome", template.observableOutcome)
                put("minutes", template.estimatedMinutes)
                put("project_id", template.projectId ?: JSONObject.NULL)
                put("day", template.dayOfWeek)
                put("hour", template.hour)
                put("minute", template.minute)
                put("time_zone", template.timeZone)
                put("active_task_id", template.activeTaskId ?: JSONObject.NULL)
                put("current_due_at", template.currentDueAt ?: JSONObject.NULL)
                put("enabled", template.enabled)
            })
        }
        context.getSharedPreferences(PREFERENCES, Context.MODE_PRIVATE)
            .edit()
            .putString(TEMPLATES, array.toString())
            .apply()
    }
}
