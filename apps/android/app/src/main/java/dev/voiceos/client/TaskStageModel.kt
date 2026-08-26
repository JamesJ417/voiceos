package dev.voiceos.client

data class TaskStage(
    val title: String,
    val owner: String,
    val status: String,
    val detail: String,
)

object TaskStageModel {
    fun stages(task: VoiceTask): List<TaskStage> {
        if (task.steps.isNotEmpty()) {
            return task.steps.map { step ->
                TaskStage(
                    title = step.title.ifBlank { "Untitled stage" },
                    owner = step.owner.ifBlank { "shared" },
                    status = normalizeStatus(step.status),
                    detail = stageDetail(step.status, step.owner),
                )
            }
        }
        val workStatus = when (task.status) {
            "completed" -> "completed"
            "active" -> "active"
            "blocked" -> "blocked"
            else -> "ready"
        }
        val reviewStatus = when {
            task.status == "completed" -> "completed"
            task.progressLane == "review" -> "active"
            else -> "ready"
        }
        return listOf(
            TaskStage("Define the outcome", "shared", "completed", "The finish line is captured and ready to guide the work."),
            TaskStage(task.nextUserAction.ifBlank { "Complete the planned work" }, ownerFor(task), workStatus, stageDetail(workStatus, ownerFor(task))),
            TaskStage("Review and confirm the outcome", "user", reviewStatus, stageDetail(reviewStatus, "user")),
        )
    }

    fun progressPercent(stages: List<TaskStage>): Int =
        if (stages.isEmpty()) 0 else stages.count { it.status == "completed" } * 100 / stages.size

    fun nextAction(task: VoiceTask): String = when (task.progressLane) {
        "vic_working" -> task.nextVicAction.ifBlank { "VIC is completing the active stage" }
        "review", "needs_me" -> task.nextUserAction.ifBlank { "Review the active stage with VIC" }
        else -> task.nextUserAction.ifBlank {
            stages(task).firstOrNull { it.status != "completed" }?.title ?: "Task complete"
        }
    }

    fun statusLabel(status: String): String = when (normalizeStatus(status)) {
        "completed" -> "COMPLETE"
        "active" -> "IN PROGRESS"
        "blocked" -> "BLOCKED"
        else -> "UP NEXT"
    }

    fun ownerLabel(owner: String): String = when (owner.lowercase()) {
        "vic", "hermes", "agent" -> "VIC"
        "user", "me", "you" -> "YOU"
        else -> "SHARED"
    }

    private fun ownerFor(task: VoiceTask): String = when (task.progressLane) {
        "vic_working" -> "vic"
        "review", "needs_me" -> "user"
        else -> "shared"
    }

    private fun normalizeStatus(status: String): String = when (status.lowercase()) {
        "done", "complete", "completed", "succeeded" -> "completed"
        "active", "running", "in_progress", "working" -> "active"
        "blocked", "failed" -> "blocked"
        else -> "ready"
    }

    private fun stageDetail(status: String, owner: String): String = when (normalizeStatus(status)) {
        "completed" -> "Finished by ${ownerLabel(owner)}. This stage is closed."
        "active" -> "Currently owned by ${ownerLabel(owner)}. Work is underway."
        "blocked" -> "Stopped at this stage. ${ownerLabel(owner)} must resolve the blocker."
        else -> "Queued for ${ownerLabel(owner)} after the preceding stage is complete."
    }
}
