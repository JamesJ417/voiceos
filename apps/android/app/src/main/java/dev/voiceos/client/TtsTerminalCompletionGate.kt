package dev.voiceos.client

/** Prevents duplicate TTS terminal callbacks from completing one utterance twice. */
internal class TtsTerminalCompletionGate {
    private var completed = false

    fun tryComplete(): Boolean {
        if (completed) return false
        completed = true
        return true
    }

    fun reset() {
        completed = false
    }
}
