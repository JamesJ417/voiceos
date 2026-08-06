package dev.voiceos.client

/** Keeps a conversational session recoverable when the user goes quiet. */
object ConversationIdlePolicy {
    enum class Action {
        KEEP_LISTENING,
        PAUSE,
    }

    fun afterSilence(silentAttempts: Int, attemptsBeforePause: Int): Action =
        if (silentAttempts >= attemptsBeforePause) Action.PAUSE else Action.KEEP_LISTENING
}
