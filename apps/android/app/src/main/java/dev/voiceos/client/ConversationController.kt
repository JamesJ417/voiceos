package dev.voiceos.client

internal enum class ConversationPhase {
    STOPPED,
    STARTING,
    LISTENING,
    PROCESSING,
    SPEAKING,
    RECONNECTING,
    PAUSED,
}

internal enum class ConversationStopReason(val wireValue: String) {
    USER_UI("ui_end"),
    USER_VOICE("voice_end"),
    SERVICE_DESTROYED("android_destroyed"),
    ;

    companion object {
        fun fromWire(value: String?): ConversationStopReason = entries.firstOrNull {
            it.wireValue == value
        } ?: USER_UI
    }
}

internal sealed interface ConversationEvent {
    data object Start : ConversationEvent
    data object ListenerReady : ConversationEvent
    data object SpeechDetected : ConversationEvent
    data object TurnSubmitted : ConversationEvent
    data object RetryScheduled : ConversationEvent
    data object ResponseStarted : ConversationEvent
    data object ResponseFinished : ConversationEvent
    data object Pause : ConversationEvent
    data object Resume : ConversationEvent
    data class End(val reason: ConversationStopReason) : ConversationEvent
}

/** Pure lifecycle authority for a continuous VIC conversation. */
internal class ConversationController {
    var phase: ConversationPhase = ConversationPhase.STOPPED
        private set

    var lastStopReason: ConversationStopReason? = null
        private set

    val active: Boolean get() = phase != ConversationPhase.STOPPED
    val paused: Boolean get() = phase == ConversationPhase.PAUSED
    val speaking: Boolean get() = phase == ConversationPhase.SPEAKING
    val requestInFlight: Boolean get() = phase == ConversationPhase.PROCESSING

    fun dispatch(event: ConversationEvent): ConversationPhase {
        phase = when (event) {
            ConversationEvent.Start,
            ConversationEvent.Resume -> ConversationPhase.STARTING

            ConversationEvent.ListenerReady,
            ConversationEvent.SpeechDetected -> if (active && !paused) {
                ConversationPhase.LISTENING
            } else {
                phase
            }

            ConversationEvent.TurnSubmitted -> if (active && !paused) {
                ConversationPhase.PROCESSING
            } else {
                phase
            }

            ConversationEvent.RetryScheduled -> if (active && !paused) {
                ConversationPhase.RECONNECTING
            } else {
                phase
            }

            ConversationEvent.ResponseStarted -> if (active && !paused) {
                ConversationPhase.SPEAKING
            } else {
                phase
            }

            ConversationEvent.ResponseFinished -> if (active && !paused) {
                ConversationPhase.STARTING
            } else {
                phase
            }

            ConversationEvent.Pause -> if (active) ConversationPhase.PAUSED else phase
            is ConversationEvent.End -> {
                lastStopReason = event.reason
                ConversationPhase.STOPPED
            }
        }
        if (event is ConversationEvent.Start || event is ConversationEvent.Resume) {
            lastStopReason = null
        }
        return phase
    }
}

internal data class PendingConversationTurn(
    val requestId: String,
    val text: String,
    val retryAttempt: Int = 0,
)

internal object ConversationRetryPolicy {
    private const val MAX_DELAY_MILLIS = 30_000L

    fun delayMillis(retryAttempt: Int): Long {
        val exponent = (retryAttempt - 1).coerceIn(0, 5)
        return (1_000L * (1L shl exponent)).coerceAtMost(MAX_DELAY_MILLIS)
    }
}
