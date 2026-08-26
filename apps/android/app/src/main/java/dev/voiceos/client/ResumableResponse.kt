package dev.voiceos.client

/** Holds an unfinished VIC response until the conversation is explicitly resumed. */
internal class ResumableResponse(initialResponse: String? = null) {
    private var response = initialResponse?.takeIf(String::isNotBlank)

    val pending: String?
        get() = response

    fun pause(currentSpeech: String?, isSpeaking: Boolean): Boolean {
        if (!isSpeaking) return false
        response = currentSpeech?.takeIf(String::isNotBlank)
        return response != null
    }

    /** Returns the response to replay without discarding its durable copy. */
    fun peekForResume(): String? = response

    /** Clears the durable copy only after TextToSpeech confirms playback completed. */
    fun markReplayComplete() {
        clear()
    }

    fun restore(savedResponse: String?) {
        if (response == null) response = savedResponse?.takeIf(String::isNotBlank)
    }

    fun clear() {
        response = null
    }
}
