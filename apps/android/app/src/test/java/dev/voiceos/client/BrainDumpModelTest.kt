package dev.voiceos.client

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class BrainDumpModelTest {
    @Test
    fun `repeated unfinished task is recognized and not selected`() {
        val review = BrainDumpModel.review(
            "I need to call the dentist",
            listOf(task("dentist", "Call the dentist")),
        )

        val proposal = review.proposals.single()
        assertEquals(BrainDumpAction.DUPLICATE, proposal.action)
        assertFalse(proposal.selectedByDefault)
        assertEquals("dentist", proposal.existingTaskId)
    }

    @Test
    fun `new concrete work becomes a selected task`() {
        val review = BrainDumpModel.review("I need to book the car inspection", emptyList())

        val proposal = review.proposals.single()
        assertEquals(BrainDumpAction.CREATE, proposal.action)
        assertTrue(proposal.selectedByDefault)
        assertEquals("Book the car inspection", proposal.title)
    }

    @Test
    fun `new information updates a related unfinished task`() {
        val review = BrainDumpModel.review(
            "I need to update the budget report with the new deadline",
            listOf(task("budget", "Finish budget report", outcome = "Budget report submitted")),
        )

        assertEquals(BrainDumpAction.UPDATE, review.proposals.single().action)
        assertEquals("budget", review.proposals.single().existingTaskId)
    }

    @Test
    fun `vic calls out an omitted high importance task`() {
        val review = BrainDumpModel.review(
            "I need to buy printer paper",
            listOf(task("taxes", "Submit overdue taxes", importance = "high", minutes = 90)),
        )

        assertTrue(review.callouts.any { it.contains("Submit overdue taxes") })
    }

    @Test
    fun `vic calls out choosing easy work while large work waits`() {
        val review = BrainDumpModel.review(
            "I need to call the pharmacy",
            listOf(task("proposal", "Write client proposal", minutes = 120)),
        )

        assertTrue(review.callouts.any { it.contains("size is a reason to shrink") })
    }

    @Test
    fun `priority answers clear questions and promote the matching item`() {
        val transcript = "I need to call the pharmacy. I need to prepare the client presentation"
        val initial = BrainDumpModel.review(transcript, emptyList())
        assertTrue(initial.questions.isNotEmpty())

        val answers = initial.questions.associate { question ->
            question.id to when (question.id) {
                "consequence" -> "The client presentation has the biggest consequence"
                "avoidance" -> "I am avoiding the client presentation"
                else -> "The client presentation is due Friday"
            }
        }
        val reviewed = BrainDumpModel.review(transcript, emptyList(), answers)

        assertTrue(reviewed.questions.isEmpty())
        assertEquals(
            "high",
            reviewed.proposals.first { it.title.contains("client presentation", ignoreCase = true) }.importance,
        )
    }

    private fun task(
        id: String,
        title: String,
        outcome: String = "Done: $title",
        importance: String = "normal",
        minutes: Int = 20,
    ) = VoiceTask(
        id = id,
        title = title,
        observableOutcome = outcome,
        estimatedMinutes = minutes,
        importance = importance,
        status = "ready",
        updatedAt = "2026-08-25T00:00:00Z",
    )
}
