package dev.voiceos.client

import java.time.DayOfWeek
import java.time.ZoneId
import java.time.ZonedDateTime
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class WeeklyTaskModelTest {
    private val zone = ZoneId.of("America/New_York")

    @Test
    fun `monday payroll gets the next monday one pm deadline`() {
        val now = ZonedDateTime.of(2026, 8, 25, 10, 0, 0, 0, zone)
        val due = WeeklyTaskModel.firstDue(
            WeeklyTaskDraft(DayOfWeek.MONDAY.value, 13, 0, zone.id),
            now,
        )

        assertTrue(due.startsWith("2026-08-31T13:00"))
    }

    @Test
    fun `same day future time remains this week`() {
        val now = ZonedDateTime.of(2026, 8, 24, 9, 0, 0, 0, zone)
        val due = WeeklyTaskModel.firstDue(
            WeeklyTaskDraft(DayOfWeek.MONDAY.value, 13, 0, zone.id),
            now,
        )

        assertTrue(due.startsWith("2026-08-24T13:00"))
    }

    @Test
    fun `completion advances exactly one weekly occurrence`() {
        val template = WeeklyTaskTemplate(
            id = "payroll",
            title = "Submit payroll",
            observableOutcome = "Payroll submitted",
            estimatedMinutes = 30,
            projectId = null,
            dayOfWeek = DayOfWeek.MONDAY.value,
            hour = 13,
            minute = 0,
            timeZone = zone.id,
            activeTaskId = "task-1",
            currentDueAt = "2026-08-24T13:00:00-04:00",
        )
        val now = ZonedDateTime.of(2026, 8, 24, 12, 0, 0, 0, zone)

        assertTrue(WeeklyTaskModel.nextDue(template, now).startsWith("2026-08-31T13:00"))
    }

    @Test
    fun `schedule label is clear`() {
        assertEquals(
            "EVERY MONDAY • DUE 1:00 PM",
            WeeklyTaskModel.scheduleLabel(DayOfWeek.MONDAY.value, 13, 0),
        )
    }
}
