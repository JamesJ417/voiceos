package dev.voiceos.client

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class ResumableResponseTest {
    @Test
    fun pauseDuringSpeechPreservesTheCurrentResponseForResume() {
        val response = ResumableResponse()

        assertTrue(response.pause(currentSpeech = "The answer was interrupted.", isSpeaking = true))

        assertEquals("The answer was interrupted.", response.pending)
        assertEquals("The answer was interrupted.", response.peekForResume())
        assertEquals("The answer was interrupted.", response.pending)
    }

    @Test
    fun pauseOutsideSpeechDoesNotCreateAReplay() {
        val response = ResumableResponse()

        assertFalse(response.pause(currentSpeech = "Already complete.", isSpeaking = false))

        assertNull(response.pending)
    }

    @Test
    fun restoredPendingResponseReplaysAfterServiceRecreation() {
        val beforeRecreation = ResumableResponse()
        beforeRecreation.pause(currentSpeech = "Continue this reply.", isSpeaking = true)
        val persisted = beforeRecreation.pending
        val afterRecreation = ResumableResponse(persisted)

        assertEquals("Continue this reply.", afterRecreation.peekForResume())
        assertEquals("Continue this reply.", afterRecreation.pending)
    }

    @Test
    fun responseRemainsDurableUntilReplayCompletes() {
        val response = ResumableResponse("Finish this interrupted reply.")

        assertEquals("Finish this interrupted reply.", response.peekForResume())
        assertEquals("Finish this interrupted reply.", response.pending)

        response.markReplayComplete()

        assertNull(response.pending)
    }
}
