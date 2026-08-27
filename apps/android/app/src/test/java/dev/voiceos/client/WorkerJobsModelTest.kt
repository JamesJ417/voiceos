package dev.voiceos.client

import java.time.Instant
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class WorkerJobsModelTest {
    private val now = Instant.parse("2026-08-27T12:00:00Z")

    @Test fun groupsWorkersAndMarksHungUpdates() {
        val cards = WorkerJobsModel.cards(listOf(
            AgentWorkerItem(2, "a", "running", "Active", null, null, taskTitle = "Ship", updatedAt = "2026-08-27T11:59:00Z"),
            AgentWorkerItem(1, "b", "completed", "Done", null, null, updatedAt = "2026-08-27T11:00:00Z"),
            AgentWorkerItem(3, "c", "running", "Hung", null, null, updatedAt = "2026-08-27T10:00:00Z"),
        ), now)
        assertEquals(listOf("failed", "active", "completed"), cards.map { it.lane })
        assertTrue(cards.first().status.contains("hung"))
        assertTrue(cards.first().blocker.contains("No worker update"))
        assertTrue(cards[1].association.contains("Ship"))
    }

    @Test fun missingTimestampIsVisibleNotInvented() {
        val card = WorkerJobsModel.cards(listOf(AgentWorkerItem(1, "x", "queued", "", null, null)), now).single()
        assertEquals("waiting", card.lane)
        assertEquals("VIC background worker", card.title)
        assertEquals("Updated time unavailable", card.timing)
    }

    @Test fun retainsMoreThanTheFormerEightWorkerLimit() {
        val model = AgentVisibilityModel()
        repeat(9) { index ->
            model.updateWorker(AgentWorkerItem(index.toLong(), "worker-$index", "running", "Worker $index", null, null))
        }
        assertEquals(9, model.workers.size)
    }
}
