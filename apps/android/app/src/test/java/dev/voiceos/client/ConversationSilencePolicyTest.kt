package dev.voiceos.client

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class ConversationSilencePolicyTest {
    @Test
    fun repeatedRecognizerCallbacksDoNotEndConversationBeforeDeadline() {
        var now = 1_000L
        val policy = ConversationSilencePolicy(20_000L) { now }

        repeat(10) {
            now += 1_999L
            assertFalse(policy.shouldEndConversation())
        }
    }

    @Test
    fun conversationEndsAtInactivityDeadline() {
        var now = 5_000L
        val policy = ConversationSilencePolicy(20_000L) { now }

        now += 19_999L
        assertFalse(policy.shouldEndConversation())
        now += 1L
        assertTrue(policy.shouldEndConversation())
    }

    @Test
    fun speechActivityStartsANewInactivityWindow() {
        var now = 100L
        val policy = ConversationSilencePolicy(20_000L) { now }

        now += 15_000L
        policy.markActivity()
        now += 15_000L
        assertFalse(policy.shouldEndConversation())
        now += 5_000L
        assertTrue(policy.shouldEndConversation())
    }

    @Test
    fun clockRollbackDoesNotCreateNegativeIdleDuration() {
        var now = 500L
        val policy = ConversationSilencePolicy(20_000L) { now }

        now = 100L
        assertFalse(policy.shouldEndConversation())
    }
}
