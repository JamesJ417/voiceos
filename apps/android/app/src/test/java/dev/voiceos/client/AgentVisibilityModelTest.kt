package dev.voiceos.client

import org.junit.Assert.assertEquals
import org.junit.Test

class AgentVisibilityModelTest {
    @Test
    fun newestActivityAppearsFirstAndDraftingUpdatesCollapse() {
        val model = AgentVisibilityModel()

        model.updateActivity(activity("1", "tool.started", "Started search"))
        model.updateActivity(activity("2", "response.drafting", "Drafting"))
        model.updateActivity(activity("3", "response.drafting", "Still drafting"))

        assertEquals(listOf("3", "1"), model.activities.map { it.id })
    }

    @Test
    fun completedWorkerReplacesItsRunningCard() {
        val model = AgentVisibilityModel()

        model.updateWorker(worker("hermes-1", "running"))
        model.updateWorker(worker("hermes-1", "completed"))

        assertEquals(1, model.workers.size)
        assertEquals("completed", model.workers.single().status)
        assertEquals(0, model.runningWorkerCount())
    }

    @Test
    fun activityAndWorkersAreBounded() {
        val model = AgentVisibilityModel(limit = 2)

        repeat(3) { index ->
            model.updateActivity(activity(index.toString(), "tool.completed", "Step $index"))
            model.updateWorker(worker(index.toString(), "running"))
        }

        assertEquals(listOf("2", "1"), model.activities.map { it.id })
        assertEquals(listOf("2", "1"), model.workers.map { it.id })
        assertEquals(2, model.runningWorkerCount())
    }

    private fun activity(id: String, phase: String, label: String) = AgentActivityItem(
        eventId = id.toLong(),
        id = id,
        phase = phase,
        label = label,
        detail = null,
        sessionId = null,
    )

    private fun worker(id: String, status: String) = AgentWorkerItem(
        eventId = id.substringAfterLast("-").toLongOrNull() ?: 1L,
        id = id,
        status = status,
        label = "Hermes worker",
        detail = null,
        sessionId = null,
    )
}
