package dev.voiceos.client

import org.junit.Assert.*
import org.junit.Test

class VoiceInteractionModeTest {
    @Test fun ownershipIsExclusiveUntilReleased() {
        val ownership = VoiceOwnership()
        assertTrue(ownership.claim(VoiceInteractionMode.RAMBLE))
        assertFalse(ownership.claim(VoiceInteractionMode.CONVERSATION))
        ownership.release()
        assertTrue(ownership.claim(VoiceInteractionMode.CONVERSATION))
    }

    @Test fun noneCannotOwnAudio() {
        assertFalse(VoiceOwnership().claim(VoiceInteractionMode.NONE))
    }
}
