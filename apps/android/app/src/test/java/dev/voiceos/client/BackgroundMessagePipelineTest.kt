package dev.voiceos.client


import org.junit.Assert.*
import org.junit.Test

class BackgroundMessagePipelineTest {
    @Test fun completedWorkerResultIsClassifiedAndGetsStableId() {
        assertTrue(BackgroundMessagePipeline.isCompletedWorkerResult("agent.worker.updated", "completed"))
        assertEquals("background:42", BackgroundMessagePipeline.stableId(42L))
    }

    @Test fun unrelatedEventIsNotACompletion() {
        assertFalse(BackgroundMessagePipeline.isCompletedWorkerResult("task.changed", "completed"))
    }

    @Test fun notificationIsAllowedForNewDeliveryButNeverReadAloudAutomatically() {
        assertTrue(BackgroundMessagePipeline.shouldNotify(activeConversation = true, alreadyDelivered = false))
        assertFalse(BackgroundMessagePipeline.shouldNotify(activeConversation = false, alreadyDelivered = true))
        assertFalse(BackgroundMessagePipeline.shouldReadAloud(activeConversation = true))
    }

    @Test fun inboxUnreadProjectionAndExplicitRead() {
        val messages = listOf(BackgroundMessage("a", "one"), BackgroundMessage("b", "two", read = true))
        assertEquals(1, BackgroundMessageInboxModel.unreadCount(messages))
        assertEquals(0, BackgroundMessageInboxModel.unreadCount(BackgroundMessageInboxModel.markRead(messages, "a")))
    }
}
