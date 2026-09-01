package dev.voiceos.client

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class TtsResponseChunkerTest {
    @Test
    fun longResponseIsQueuedInOrderWithoutLoss() {
        val response = (1..800).joinToString(" ") { "word$it" }
        val chunks = TtsResponseChunker.split(response, 180)

        assertTrue(chunks.size > 2)
        assertTrue(chunks.all { it.length <= 180 })
        assertEquals(response, chunks.joinToString(" "))
    }

    @Test
    fun sentenceBoundaryIsPreferred() {
        val chunks = TtsResponseChunker.split(
            "First complete sentence. Second sentence is deliberately longer than the limit.",
            40,
        )
        assertEquals("First complete sentence.", chunks.first())
    }
}
