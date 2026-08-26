package dev.voiceos.client

import org.junit.Assert.assertEquals
import org.junit.Test

class TaskStageModelTest {
    private fun task(
        status: String = "active",
        lane: String = "shared",
        steps: List<VoiceTaskStep> = emptyList(),
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
}
