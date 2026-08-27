package dev.voiceos.client

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class JobBoardModelTest {
    @Test fun groupsLanesAndFormatsAuthoritativeFields() {
        val cards = JobBoardModel.cards(listOf(
            VoiceTask("1", title = "Ship board", observableOutcome = "", estimatedMinutes = 10, status = "active", updatedAt = "", progressLane = "vic_working", vicStatus = "running", completedSteps = 2, totalSteps = 4, openBlockers = 1, nextVicAction = "Wire refresh"),
            VoiceTask("2", title = "Review", observableOutcome = "", estimatedMinutes = 5, status = "active", updatedAt = "", progressLane = "needs_you"),
        ))
        assertEquals(listOf("needs_you", "vic_working"), cards.map { it.lane })
        assertTrue(JobBoardModel.format(cards[1]).contains("2/4 complete"))
        assertTrue(JobBoardModel.format(cards[1]).contains("Wire refresh"))
    }

    @Test fun missingOptionalFieldsAreSafe() {
        val card = JobBoardModel.cards(listOf(VoiceTask("1", title = "", observableOutcome = "", estimatedMinutes = 0, status = "active", updatedAt = "", progressLane = " "))).single()
        assertEquals("shared", card.lane)
        assertEquals("Untitled job", card.title)
        assertEquals("No blockers", card.blockers)
        assertEquals("No next action recorded", card.nextAction)
        assertEquals("not analyzed", card.workerStatus)
    }
}
