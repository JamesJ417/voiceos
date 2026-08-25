package dev.voiceos.client

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class ArticleListeningModelTest {
    @Test
    fun `summary is always listenable when a page has no article text`() {
        val narration = ArticleListeningModel.prepare(
            "A new AI model",
            "OpenAI",
            "The official summary explains what launched and why it matters.",
            "Menu\nSign in",
        )

        assertTrue(narration.contains("From OpenAI."))
        assertTrue(narration.contains("A new AI model"))
        assertTrue(narration.contains("official summary"))
        assertFalse(narration.contains("Sign in"))
    }

    @Test
    fun `full readable article replaces the short summary`() {
        val body = (1..80).joinToString(" ") { "articleword$it" }
        val narration = ArticleListeningModel.prepare("Title", "DeepMind", "Short summary", body)

        assertTrue(narration.contains("articleword80"))
        assertFalse(narration.contains("Short summary"))
    }

    @Test
    fun `long narration is divided into safe speech chunks`() {
        val text = (1..700).joinToString(" ") { "word$it" }
        val chunks = ArticleListeningModel.chunks(text, maxChars = 300)

        assertTrue(chunks.size > 1)
        assertTrue(chunks.all { it.length <= 300 })
        assertEquals("word1", chunks.first().substringBefore(' '))
        assertTrue(chunks.last().endsWith("word700"))
    }
}
