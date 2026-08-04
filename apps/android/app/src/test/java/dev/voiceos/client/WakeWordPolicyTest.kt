package dev.voiceos.client

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class WakeWordPolicyTest {
    @Test
    fun acceptsConfidentKeywordOutsideCooldown() {
        assertTrue(WakeWordPolicy.shouldActivate("HEY_VIC", 0.08f, 20_000L, 0L, false))
    }

    @Test
    fun rejectsSilenceCooldownAndActiveConversation() {
        assertFalse(WakeWordPolicy.shouldActivate("HEY_VIC", 0.001f, 20_000L, 0L, false))
        assertFalse(WakeWordPolicy.shouldActivate("HEY_VIC", 0.08f, 5_000L, 0L, false))
        assertFalse(WakeWordPolicy.shouldActivate("HEY_VIC", 0.08f, 20_000L, 0L, true))
        assertFalse(WakeWordPolicy.shouldActivate("", 0.08f, 20_000L, 0L, false))
    }
}
