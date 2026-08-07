package dev.voiceos.client

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class ConversationFloorPolicyTest {
    private fun floor(revision: Long, holder: String?) = ConversationFloor(
        conversationId = "conversation",
        holderDeviceId = holder,
        holderDisplayName = "Panel",
        phase = "listening",
        partialTranscript = null,
        responseText = null,
        revision = revision,
        active = true,
    )

    @Test
    fun stale_replayed_floor_event_does_not_end_new_session() {
        assertFalse(ConversationFloorPolicy.shouldYield(true, 12, floor(11, "panel"), "pixel"))
        assertFalse(ConversationFloorPolicy.shouldYield(true, 12, floor(12, "panel"), "pixel"))
    }

    @Test
    fun newer_floor_transfer_to_another_device_ends_local_session() {
        assertTrue(ConversationFloorPolicy.shouldYield(true, 12, floor(13, "panel"), "pixel"))
    }

    @Test
    fun own_newer_floor_event_does_not_end_local_session() {
        assertFalse(ConversationFloorPolicy.shouldYield(true, 12, floor(13, "pixel"), "pixel"))
    }
}
