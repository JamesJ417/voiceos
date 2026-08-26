package dev.voiceos.client

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class RecognitionWatchdogPolicyTest {
    @Test
    fun quietSpeechRequestsFinalizationBeforeTheHardRecognizerLimit() {
        val quietMillis = RecognitionWatchdogPolicy.partialResultQuietMillis(
            "Please tell me today's date",
        )
        assertTrue(quietMillis >= 4_000L)
        assertTrue(
            RecognitionWatchdogPolicy.FINAL_RESULT_GRACE_MILLIS <
                RecognitionWatchdogPolicy.RECOGNIZER_HARD_LIMIT_MILLIS,
        )
        assertTrue(
            quietMillis +
                RecognitionWatchdogPolicy.FINAL_RESULT_GRACE_MILLIS <
                RecognitionWatchdogPolicy.RECOGNIZER_HARD_LIMIT_MILLIS,
        )
    }

    @Test
    fun incompletePhraseGetsLongerThinkingPause() {
        assertEquals(
            8_000L,
            RecognitionWatchdogPolicy.partialResultQuietMillis("Is there a"),
        )
        assertTrue(
            RecognitionWatchdogPolicy.SPEECH_INPUT_POSSIBLY_COMPLETE_SILENCE_MILLIS > 4_000L,
        )
        assertTrue(
            RecognitionWatchdogPolicy.SPEECH_INPUT_COMPLETE_SILENCE_MILLIS >=
                RecognitionWatchdogPolicy.SPEECH_INPUT_POSSIBLY_COMPLETE_SILENCE_MILLIS,
        )
    }

    @Test
    fun shortCompletePhraseStillGetsNaturalPause() {
        assertEquals(
            6_000L,
            RecognitionWatchdogPolicy.partialResultQuietMillis("Tell me why"),
        )
    }

    @Test
    fun tinyFragmentAfterHardLimitIsRetriedOnPlatformRecognizer() {
        assertTrue(
            RecognitionWatchdogPolicy.hardLimitPartialNeedsPlatformRetry(
                RecognitionWatchdogPolicy.HARD_LIMIT_REASON,
                "Is the",
            ),
        )
        assertTrue(
            !RecognitionWatchdogPolicy.hardLimitPartialNeedsPlatformRetry(
                RecognitionWatchdogPolicy.PARTIAL_QUIET_REASON,
                "Is the",
            ),
        )
        assertTrue(
            !RecognitionWatchdogPolicy.hardLimitPartialNeedsPlatformRetry(
                RecognitionWatchdogPolicy.HARD_LIMIT_REASON,
                "Is there a way to continue",
            ),
        )
    }
}
