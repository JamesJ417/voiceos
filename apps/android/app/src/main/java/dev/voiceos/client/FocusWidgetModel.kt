package dev.voiceos.client

import java.time.OffsetDateTime

data class FocusWidgetSelection(
    val choices: List<VoiceTask>,
    val parkedCount: Int,
) {
    val primary: VoiceTask?
        get() = choices.firstOrNull()
}

object FocusWidgetModel {
    fun select(tasks: List<VoiceTask>): FocusWidgetSelection {
        val open = tasks.filter { it.status !in setOf("completed", "cancelled") }
        val ranked = open.sortedWith(
            compareBy<VoiceTask> { if (it.status == "active") 0 else 1 }
                .thenBy {
                    when (it.progressLane) {
                        "needs_me" -> 0
                        "review" -> 1
                        "shared" -> 2
                        "vic_working" -> 3
                        else -> 2
                    }
                }
                .thenBy { dueRank(it.dueAt) }
                .thenBy { when (it.importance) { "critical" -> 0; "high" -> 1; "normal" -> 2; else -> 3 } }
                .thenBy { it.estimatedMinutes.coerceAtLeast(1) },
        )
        val choices = ranked.take(MAX_VISIBLE_CHOICES)
        return FocusWidgetSelection(choices, (ranked.size - choices.size).coerceAtLeast(0))
    }

    fun recommendationReason(task: VoiceTask): String = when {
        task.status == "active" -> "KEEP YOUR MOMENTUM"
        task.progressLane == "needs_me" -> "NEEDS YOUR INPUT"
        task.progressLane == "review" -> "READY FOR REVIEW"
        task.estimatedMinutes <= 5 -> "QUICK WIN"
        else -> "BEST NEXT MOVE"
    }

    fun nextAction(task: VoiceTask): String = when (task.progressLane) {
        "needs_me", "review" -> task.nextUserAction
        "vic_working" -> task.nextVicAction.ifBlank { "VIC is preparing the next step" }
        else -> task.nextUserAction
    }.ifBlank {
        task.steps.firstOrNull { it.status != "completed" && it.owner != "vic" }?.title
            ?: task.observableOutcome
    }.ifBlank { "Talk with VIC to choose the first small step" }

    private fun dueRank(value: String?): Long = value
        ?.let { runCatching { OffsetDateTime.parse(it).toInstant().epochSecond }.getOrNull() }
        ?: Long.MAX_VALUE

    private const val MAX_VISIBLE_CHOICES = 3
}
