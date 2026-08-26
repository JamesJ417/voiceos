package dev.voiceos.client

import java.time.Instant
import org.junit.Assert.assertEquals
import org.junit.Test

class TaskStageModelTest {
    private fun task(
        status: String = "active",
        lane: String = "shared",
        steps: List<VoiceTaskStep> = emptyList(),
        handoffs: List<VoiceTaskHandoff> = emptyList(),
    ) = VoiceTask(
        id = "task-1",
        title = "Ship the task view",
        observableOutcome = "Task stages are visible",
        estimatedMinutes = 30,
        status = status,
        updatedAt = "2026-08-26T00:00:00Z",
        progressLane = lane,
        nextUserAction = "Review the task view",
        nextVicAction = "Build the task view",
        steps = steps,
        handoffs = handoffs,
    )

    @Test
    fun `backend steps become numbered detail stages without losing status`() {
        val stages = TaskStageModel.stages(task(steps = listOf(
            VoiceTaskStep("Plan", "vic", "completed"),
            VoiceTaskStep("Build", "vic", "active"),
            VoiceTaskStep("Review", "user", "ready"),
        )))

        assertEquals(listOf("completed", "active", "ready"), stages.map { it.status })
        assertEquals(33, TaskStageModel.progressPercent(stages))
        assertEquals("VIC", TaskStageModel.ownerLabel(stages[1].owner))
    }

    @Test
    fun `tasks without backend steps get an honest lifecycle breakdown`() {
        val stages = TaskStageModel.stages(task(lane = "review"))

        assertEquals(3, stages.size)
        assertEquals("completed", stages[0].status)
        assertEquals("active", stages[2].status)
        assertEquals("Review the task view", TaskStageModel.nextAction(task(lane = "review")))
    }

    @Test
    fun `completed fallback lifecycle reports full progress`() {
        val stages = TaskStageModel.stages(task(status = "completed"))

        assertEquals(100, TaskStageModel.progressPercent(stages))
    }

    @Test
    fun `pending VIC handoff is always surfaced as an urgent user follow up`() {
        val followUp = TaskStageModel.followUp(task(handoffs = listOf(
            VoiceTaskHandoff(
                id = "handoff-1",
                fromOwner = "vic",
                toOwner = "user",
                kind = "review",
                summary = "Review the result",
                status = "pending",
                createdAt = "2026-08-26T00:00:00Z",
            ),
        )), Instant.parse("2026-08-26T01:00:00Z"))

        assertEquals("ACCEPT VIC HANDOFF", followUp?.label)
        assertEquals(true, followUp?.urgent)
    }

    @Test
    fun `unchanged active stage becomes a visible follow up after one day`() {
        val followUp = TaskStageModel.followUp(task(steps = listOf(
            VoiceTaskStep("Build", "vic", "active", id = "step-1", updatedAt = "2026-08-24T00:00:00Z"),
        )), Instant.parse("2026-08-26T00:00:00Z"))

        assertEquals("FOLLOW UP • NO MOVE IN 24H", followUp?.label)
        assertEquals("vic", followUp?.owner)
    }
}
