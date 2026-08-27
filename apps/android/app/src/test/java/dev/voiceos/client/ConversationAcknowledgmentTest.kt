package dev.voiceos.client

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class ConversationAcknowledgmentTest {
    @Test fun questionGetsDirectAnswerPlan() {
        val result = ConversationAcknowledgment.forRequest("How do I refresh the feed?")
        assertTrue(result.text.startsWith("Got it."))
        assertTrue(result.text.contains("direct answer"))
        assertEquals(1, result.estimateMinutes)
    }

    @Test fun buildRequestGetsVerificationPlan() {
        val result = ConversationAcknowledgment.forRequest("Build the Android app")
        assertTrue(result.text.contains("make the change, and verify it"))
        assertTrue(result.text.contains("minute"))
    }

    @Test fun whitespaceIsHandled() {
        val result = ConversationAcknowledgment.forRequest("  check    the logs  ")
        assertTrue(result.text.contains("safe next step"))
    }
}
