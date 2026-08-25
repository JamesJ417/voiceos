package dev.voiceos.client

import java.time.LocalDate
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class ScriptureResetModelTest {
    @Test
    fun `daily passage stays stable and rotates the next day`() {
        val date = LocalDate.of(2026, 8, 25)

        assertEquals(ScriptureResetModel.passageFor(date), ScriptureResetModel.passageFor(date))
        assertNotEquals(ScriptureResetModel.passageFor(date), ScriptureResetModel.passageFor(date.plusDays(1)))
    }

    @Test
    fun `passages always open the requested CSB translation`() {
        repeat(30) { offset ->
            val passage = ScriptureResetModel.passageFor(LocalDate.of(2026, 1, 1).plusDays(offset.toLong()))
            assertTrue(passage.csbUrl.startsWith("https://www.bible.com/bible/1713/"))
            assertTrue(passage.csbUrl.endsWith(".CSB"))
        }
    }

    @Test
    fun `vic prompt includes private thoughts and a grounded follow up boundary`() {
        val passage = ScriptureResetModel.passageFor(LocalDate.of(2026, 8, 25))
        val prompt = ScriptureResetModel.conversationPrompt(passage, "I noticed how distracted I have been.")

        assertTrue(prompt.contains(passage.reference))
        assertTrue(prompt.contains("distracted"))
        assertTrue(prompt.contains("exactly one thoughtful follow-up"))
        assertTrue(prompt.contains("do not claim to speak for God"))
    }

    @Test
    fun `blank notes ask vic to begin the reflection`() {
        val passage = ScriptureResetModel.passageFor(LocalDate.of(2026, 8, 25))
        val prompt = ScriptureResetModel.conversationPrompt(passage, "")

        assertTrue(prompt.contains("Ask me one thoughtful"))
        assertFalse(prompt.contains("My private reflection is:"))
    }
}
