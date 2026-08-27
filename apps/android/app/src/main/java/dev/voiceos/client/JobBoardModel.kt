package dev.voiceos.client

/** Read-only projection of authoritative VoiceTask records for the Jobs board. */
data class JobBoardCard(
    val id: String,
    val lane: String,
    val title: String,
    val progress: String,
    val blockers: String,
    val nextAction: String,
    val workerStatus: String,
)

object JobBoardModel {
    private val laneOrder = listOf("needs_you", "shared", "vic_working", "completed", "other")

    fun cards(tasks: List<VoiceTask>): List<JobBoardCard> = tasks.map { task ->
        val lane = task.progressLane.trim().ifBlank { "shared" }.lowercase()
        JobBoardCard(
            id = task.id,
            lane = lane,
            title = task.title.ifBlank { "Untitled job" },
            progress = "${task.completedSteps.coerceAtLeast(0)}/${task.totalSteps.coerceAtLeast(0)}",
            blockers = if (task.openBlockers > 0) "${task.openBlockers} blocker${if (task.openBlockers == 1) "" else "s"}" else "No blockers",
            nextAction = listOf(task.nextVicAction, task.nextUserAction).firstOrNull { it.isNotBlank() } ?: "No next action recorded",
            workerStatus = task.vicStatus.ifBlank { "not_analyzed" }.replace('_', ' '),
        )
    }.sortedWith(compareBy({ laneOrder.indexOf(it.lane).let { index -> if (index < 0) laneOrder.lastIndex else index } }, { it.title.lowercase() }))

    fun laneLabel(lane: String): String = lane.replace('_', ' ').split(' ').joinToString(" ") { it.replaceFirstChar(Char::uppercase) }

    fun format(card: JobBoardCard): String = "${card.title}\n${card.progress} complete • ${card.blockers}\nNext: ${card.nextAction}\nWorker: ${card.workerStatus}"
}
