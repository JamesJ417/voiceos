package dev.voiceos.client

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class FocusWidgetModelTest {
    @Test
    fun activeTaskWinsAndCompletedTasksStayHidden() {
        val selected = FocusWidgetModel.select(
            listOf(
                task("done", status = "completed", lane = "needs_me", minutes = 1),
                task("needs", lane = "needs_me", minutes = 5),
                task("active", status = "active", lane = "shared", minutes = 25),
            ),
        )

        assertEquals("active", selected.primary?.id)
        assertEquals(listOf("active", "needs"), selected.choices.map { it.id })
        assertEquals(0, selected.parkedCount)
    }

    @Test
    fun needsMeBeatsShorterSharedWork() {
        val selected = FocusWidgetModel.select(
            listOf(
                task("shared", lane = "shared", minutes = 2),
                task("mine", lane = "needs_me", minutes = 15),
            ),
        )

        assertEquals("mine", selected.primary?.id)
    }

    @Test
    fun nextActionFallsBackToFirstOpenUserStep() {
        val task = task("task", lane = "shared", minutes = 10).copy(
            observableOutcome = "Finish the setup",
            steps = listOf(
                VoiceTaskStep("Already done", "user", "completed"),
                VoiceTaskStep("Open the settings", "user", "ready"),
            ),
        )

        assertEquals("Open the settings", FocusWidgetModel.nextAction(task))
    }

    @Test
    fun emptyListHasNoPriority() {
        val selected = FocusWidgetModel.select(emptyList())

        assertNull(selected.primary)
        assertEquals(0, selected.parkedCount)
    }

    @Test
    fun showsThreeChoicesAndParksTheRest() {
        val selected = FocusWidgetModel.select(
            listOf(
                task("one", lane = "needs_me", minutes = 10),
                task("two", lane = "review", minutes = 10),
                task("three", lane = "shared", minutes = 10),
                task("four", lane = "vic_working", minutes = 10),
            ),
        )

        assertEquals(listOf("one", "two", "three"), selected.choices.map { it.id })
        assertEquals(1, selected.parkedCount)
    }

    @Test
    fun explainsWhyVicRecommendsTheFirstChoice() {
        assertEquals(
            "NEEDS YOUR INPUT",
            FocusWidgetModel.recommendationReason(task("mine", lane = "needs_me", minutes = 20)),
        )
        assertEquals(
            "QUICK WIN",
            FocusWidgetModel.recommendationReason(task("quick", lane = "shared", minutes = 5)),
        )
    }

    @Test
    fun weeklyDeadlineBeatsAnUndatedQuickTaskInTheSameLane() {
        val deadline = task("payroll", lane = "shared", minutes = 30).copy(
            dueAt = "2026-08-31T13:00:00-04:00",
            importance = "high",
        )
        val quick = task("quick", lane = "shared", minutes = 5)

        assertEquals("payroll", FocusWidgetModel.select(listOf(quick, deadline)).primary?.id)
    }

    private fun task(
        id: String,
        status: String = "ready",
        lane: String,
        minutes: Int,
    ) = VoiceTask(
        id = id,
        title = id,
        observableOutcome = "Complete $id",
        estimatedMinutes = minutes,
        status = status,
        updatedAt = "2026-08-25T00:00:00Z",
        progressLane = lane,
    )
}
