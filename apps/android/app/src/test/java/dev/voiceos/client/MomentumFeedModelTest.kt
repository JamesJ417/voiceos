package dev.voiceos.client

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class MomentumFeedModelTest {
    @Test
    fun feedStartsWithPriorityAndStaysFinite() {
        val tasks = (1..12).map { index ->
            task("task-$index", status = if (index == 3) "active" else "ready")
        }
        val cards = MomentumFeedModel.build(tasks, emptyList())

        assertEquals(MomentumCardKind.PRIORITY, cards.first().kind)
        assertEquals("task-3", cards.first().taskId)
        assertTrue(cards.size <= 8)
    }

    @Test
    fun feedContainsOnlyProvidedTasksAndInterests() {
        val task = task("mine")
        val interest = VicInterest("interest-1", "Restaurant design", 1L)
        val cards = MomentumFeedModel.build(listOf(task), listOf(interest))

        assertTrue(cards.any { it.taskId == "mine" })
        assertTrue(cards.any { it.interestId == "interest-1" })
        assertTrue(cards.all { it.taskId in setOf(null, "mine") })
    }

    private fun task(id: String, status: String = "ready") = VoiceTask(
        id = id,
        title = id,
        observableOutcome = "Finish $id",
        estimatedMinutes = 10,
        status = status,
        updatedAt = "2026-08-25T00:00:00Z",
        progressLane = "shared",
        nextUserAction = "Open $id",
    )
}
