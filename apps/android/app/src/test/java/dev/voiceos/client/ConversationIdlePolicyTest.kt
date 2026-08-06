package dev.voiceos.client

import org.junit.Assert.assertEquals
import org.junit.Test

class ConversationIdlePolicyTest {
    @Test
    fun `silence keeps the microphone cycling before the idle limit`() {
        assertEquals(
            ConversationIdlePolicy.Action.KEEP_LISTENING,
            ConversationIdlePolicy.afterSilence(5, 6),
        )
    }

    @Test
    fun `silence pauses rather than terminating at the idle limit`() {
        assertEquals(
            ConversationIdlePolicy.Action.PAUSE,
            ConversationIdlePolicy.afterSilence(6, 6),
        )
    }
}
