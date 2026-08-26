package dev.voiceos.client

internal object RecognitionWatchdogPolicy {
    const val END_OF_SPEECH_REASON = "end_of_speech"
    const val PARTIAL_QUIET_REASON = "partial_quiet"
    const val HARD_LIMIT_REASON = "hard_limit"

    private const val COMPLETE_PHRASE_QUIET_MILLIS = 4_500L
    private const val SHORT_PHRASE_QUIET_MILLIS = 6_000L
    private const val INCOMPLETE_PHRASE_QUIET_MILLIS = 8_000L
    const val SPEECH_INPUT_COMPLETE_SILENCE_MILLIS = 7_000L
    const val SPEECH_INPUT_POSSIBLY_COMPLETE_SILENCE_MILLIS = 5_500L
    const val SPEECH_INPUT_MINIMUM_LENGTH_MILLIS = 1_200L
    const val FINAL_RESULT_GRACE_MILLIS = 2_500L
    const val RECOGNIZER_HARD_LIMIT_MILLIS = 30_000L

    private val continuationWords = setOf(
        "a", "an", "and", "at", "because", "but", "for", "if", "in", "my", "of", "on",
        "or", "our", "that", "the", "to", "when", "which", "with", "your",
    )

    fun partialResultQuietMillis(partial: String): Long {
        val words = partial.trim()
            .lowercase()
            .split(Regex("\\s+"))
            .filter(String::isNotBlank)
        return when {
            words.isEmpty() -> SHORT_PHRASE_QUIET_MILLIS
            words.last().trimEnd('.', ',', '?', '!', ':', ';') in continuationWords ->
                INCOMPLETE_PHRASE_QUIET_MILLIS
            words.size <= 3 -> SHORT_PHRASE_QUIET_MILLIS
            else -> COMPLETE_PHRASE_QUIET_MILLIS
        }
    }

    fun hardLimitPartialNeedsPlatformRetry(reason: String?, partial: String?): Boolean =
        reason == HARD_LIMIT_REASON && (partial?.trim()?.length ?: 0) < 12
}
