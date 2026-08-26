package dev.voiceos.client

data class AgentActivityItem(
    val eventId: Long,
    val id: String,
    val phase: String,
    val label: String,
    val detail: String?,
    val sessionId: String?,
)

data class AgentWorkerItem(
    val eventId: Long,
    val id: String,
    val status: String,
    val label: String,
    val detail: String?,
    val sessionId: String?,
    val taskId: String? = null,
    val taskTitle: String? = null,
    val taskStatus: String? = null,
    val taskOutcome: String? = null,
    val taskProjectId: String? = null,
    val taskDueAt: String? = null,
    val taskImportance: String? = null,
    val completedSteps: Int = 0,
    val totalSteps: Int = 0,
)

class AgentVisibilityModel(private val limit: Int = 8) {
    private val recentActivity = mutableListOf<AgentActivityItem>()
    private val recentWorkers = mutableListOf<AgentWorkerItem>()

    val activities: List<AgentActivityItem>
        get() = recentActivity.toList()

    val workers: List<AgentWorkerItem>
        get() = recentWorkers.toList()

    fun updateActivity(activity: AgentActivityItem) {
        recentActivity.removeAll { it.id == activity.id }
        recentActivity.add(activity)
        recentActivity.sortByDescending { it.eventId }
        if (activity.phase == "response.drafting") {
            val newestDraft = recentActivity.firstOrNull { it.phase == "response.drafting" }
            recentActivity.removeAll { it.phase == "response.drafting" && it !== newestDraft }
        }
        trim(recentActivity)
    }

    fun updateWorker(worker: AgentWorkerItem) {
        val existing = recentWorkers.firstOrNull { it.id == worker.id }
        if (existing != null && existing.eventId > worker.eventId) return
        recentWorkers.removeAll { it.id == worker.id }
        recentWorkers.add(worker)
        recentWorkers.sortByDescending { it.eventId }
        trim(recentWorkers)
    }

    fun runningWorkerCount(): Int = recentWorkers.count { it.status.equals("running", ignoreCase = true) }

    private fun <T> trim(items: MutableList<T>) {
        while (items.size > limit.coerceAtLeast(1)) items.removeAt(items.lastIndex)
    }
}
