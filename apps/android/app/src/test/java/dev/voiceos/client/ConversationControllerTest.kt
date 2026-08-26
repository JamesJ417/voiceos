package dev.voiceos.client

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class ConversationControllerTest {
    @Test
    fun recoverableFailureNeverStopsTheConversation() {
        val controller = ConversationController()

        controller.dispatch(ConversationEvent.Start)
        controller.dispatch(ConversationEvent.ListenerReady)
        controller.dispatch(ConversationEvent.TurnSubmitted)
        controller.dispatch(ConversationEvent.RetryScheduled)

        assertTrue(controller.active)
        assertFalse(controller.requestInFlight)
        assertEquals(ConversationPhase.RECONNECTING, controller.phase)

        controller.dispatch(ConversationEvent.TurnSubmitted)
        controller.dispatch(ConversationEvent.ResponseStarted)
        controller.dispatch(ConversationEvent.ResponseFinished)
        assertEquals(ConversationPhase.STARTING, controller.phase)
    }

    @Test
    fun remoteHandoffCanPauseAndResumeWithoutEnding() {
        val controller = ConversationController()
        controller.dispatch(ConversationEvent.Start)
        controller.dispatch(ConversationEvent.Pause)

        assertTrue(controller.active)
        assertTrue(controller.paused)
        assertNull(controller.lastStopReason)

        controller.dispatch(ConversationEvent.Resume)
        assertEquals(ConversationPhase.STARTING, controller.phase)
    }

    @Test
    fun onlyExplicitEndMovesToStoppedAndRecordsReason() {
        val controller = ConversationController()
        controller.dispatch(ConversationEvent.Start)
        controller.dispatch(ConversationEvent.End(ConversationStopReason.USER_VOICE))

        assertFalse(controller.active)
        assertEquals(ConversationPhase.STOPPED, controller.phase)
        assertEquals(ConversationStopReason.USER_VOICE, controller.lastStopReason)
    }

    @Test
    fun retryBackoffIsBounded() {
        assertEquals(1_000L, ConversationRetryPolicy.delayMillis(1))
        assertEquals(2_000L, ConversationRetryPolicy.delayMillis(2))
        assertEquals(16_000L, ConversationRetryPolicy.delayMillis(5))
        assertEquals(30_000L, ConversationRetryPolicy.delayMillis(99))
    }
}
