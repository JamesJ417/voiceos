package dev.voiceos.client

/** The two mutually exclusive user-facing voice interaction contracts. */
internal enum class VoiceInteractionMode {
    NONE,
    RAMBLE,
    CONVERSATION,
}

internal class VoiceOwnership {
    var mode: VoiceInteractionMode = VoiceInteractionMode.NONE
        private set

    fun claim(requested: VoiceInteractionMode): Boolean {
        if (requested == VoiceInteractionMode.NONE || mode != VoiceInteractionMode.NONE) return false
        mode = requested
        return true
    }

    fun release() { mode = VoiceInteractionMode.NONE }
}
