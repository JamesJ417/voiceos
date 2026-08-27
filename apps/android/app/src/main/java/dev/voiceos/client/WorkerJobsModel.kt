package dev.voiceos.client

import java.time.Duration
import java.time.Instant

/** Read-only projection of worker events; no worker control is exposed here. */
data class WorkerJobCard(
    val id: String,
    val lane: String,
    val title: String,
    val association: String,
    val status: String,
    val timing: String,
    val progress: String,
    val blocker: String,
    val nextAction: String,
)

object WorkerJobsModel {
    private val active = setOf("running", "started", "active", "paused")
    private val waiting = setOf("queued", "waiting", "pending")
    private val failed = setOf("failed", "error", "lost")

    fun cards(workers: List<AgentWorkerItem>, now: Instant = Instant.now()): List<WorkerJobCard> = workers.map { worker ->
        val status = worker.status.trim().lowercase().ifBlank { "unknown" }
        val stale = status in active && ageSeconds(worker.updatedAt, now) >= 300
        val lane = when { status in failed || stale -> if (stale) "failed" else "failed"; status in waiting -> "waiting"; status in setOf("completed", "succeeded", "done") -> "completed"; else -> "active" }
        WorkerJobCard(worker.id, lane, worker.label.ifBlank { "VIC background worker" },
            listOfNotNull(worker.taskTitle?.takeIf { it.isNotBlank() }?.let { "Task: $it" }, worker.taskId?.takeIf { it.isNotBlank() }?.let { "#$it" }, worker.taskProjectId?.takeIf { it.isNotBlank() }?.let { "Project $it" }).joinToString(" • ").ifBlank { "No task/project association" },
            if (stale) "stale / possibly hung" else status.replace('_', ' '), formatAge(worker.updatedAt, now),
            if (worker.totalSteps > 0) "${worker.completedSteps.coerceAtLeast(0)}/${worker.totalSteps} steps" else "Progress not reported",
            if (stale) "No worker update for ${formatAge(worker.updatedAt, now)}" else (worker.detail?.takeIf { it.isNotBlank() } ?: "No blocker reported"),
            when { stale -> "Inspect worker and retry or recover"; status in failed -> "Review failure and retry"; status in waiting -> "Waiting for VIC"; else -> "Continue monitoring" })
    }.sortedWith(compareBy({ listOf("failed", "active", "waiting", "completed").indexOf(it.lane) }, { it.title.lowercase() }))

    fun ageSeconds(updatedAt: String?, now: Instant): Long = runCatching { Duration.between(Instant.parse(updatedAt), now).seconds }.getOrDefault(Long.MAX_VALUE)
    fun formatAge(updatedAt: String?, now: Instant): String = if (updatedAt.isNullOrBlank()) "Updated time unavailable" else runCatching { val s = ageSeconds(updatedAt, now).coerceAtLeast(0); if (s < 60) "updated ${s}s ago" else if (s < 3600) "updated ${s / 60}m ago" else "updated ${s / 3600}h ago" }.getOrDefault("Updated time unavailable")
}
