package dev.voiceos.client

enum class MomentumCardKind { PRIORITY, TASK, VIC_PREPARED, REVIEW, INTEREST, WIN }

data class MomentumCard(
    val kind: MomentumCardKind,
    val stableId: String,
    val title: String,
    val body: String,
    val taskId: String? = null,
    val interestId: String? = null,
)

object MomentumFeedModel {
    fun build(
        tasks: List<VoiceTask>,
        interests: List<VicInterest>,
        limit: Int = 8,
    ): List<MomentumCard> {
        val open = tasks.filter { it.status !in setOf("completed", "cancelled") }
        val primary = FocusWidgetModel.select(open).primary
        val cards = mutableListOf<MomentumCard>()
        primary?.let { cards += taskCard(it, MomentumCardKind.PRIORITY) }

        interests.take(2).forEachIndexed { index, interest ->
            if (index == 1 && cards.none { it.kind == MomentumCardKind.TASK }) {
                open.firstOrNull { it.id != primary?.id && it.progressLane != "vic_working" }
                    ?.let { cards += taskCard(it, MomentumCardKind.TASK) }
            }
            cards += MomentumCard(
                kind = MomentumCardKind.INTEREST,
                stableId = "interest:${interest.id}",
                title = interest.topic,
                body = "Ask VIC for one useful idea connected to this interest and your real life.",
                interestId = interest.id,
            )
        }

        open.filter { it.id != primary?.id && it.progressLane == "vic_working" }
            .take(1).forEach { cards += taskCard(it, MomentumCardKind.VIC_PREPARED) }
        open.filter { it.id != primary?.id && it.progressLane == "review" }
            .take(1).forEach { cards += taskCard(it, MomentumCardKind.REVIEW) }
        open.filter {
            it.id != primary?.id && it.progressLane !in setOf("vic_working", "review")
        }.take(2).forEach { task ->
            if (cards.none { it.taskId == task.id }) cards += taskCard(task, MomentumCardKind.TASK)
        }
        tasks.filter { it.status == "completed" }
            .sortedByDescending(VoiceTask::updatedAt)
            .take(1).forEach {
                cards += MomentumCard(
                    kind = MomentumCardKind.WIN,
                    stableId = "win:${it.id}",
                    title = it.title,
                    body = "Finished. Returning and completing both count as momentum.",
                    taskId = it.id,
                )
            }
        return cards.distinctBy(MomentumCard::stableId).take(limit.coerceIn(1, 12))
    }

    private fun taskCard(task: VoiceTask, kind: MomentumCardKind) = MomentumCard(
        kind = kind,
        stableId = "${kind.name.lowercase()}:${task.id}",
        title = task.title,
        body = when (kind) {
            MomentumCardKind.PRIORITY, MomentumCardKind.TASK -> FocusWidgetModel.nextAction(task)
            MomentumCardKind.VIC_PREPARED -> task.nextVicAction.ifBlank { "VIC is preparing this work." }
            MomentumCardKind.REVIEW -> task.nextUserAction.ifBlank { "Review VIC's prepared work." }
            else -> task.observableOutcome
        },
        taskId = task.id,
    )
}
