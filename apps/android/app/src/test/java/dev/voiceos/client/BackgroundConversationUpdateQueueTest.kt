package dev.voiceos.client

import org.junit.Assert.assertEquals
import org.junit.Test

class BackgroundConversationUpdateQueueTest {
    @Test
    fun enqueuesReportsOnceByStableEventIdAndDrainsOnlyWhenRequested() {
        val queue = BackgroundConversationUpdateQueue()

        assertEquals(0, queue.pendingCount)
        queue.enqueue("event-1", "first report")
        queue.enqueue("event-1", "duplicate report")
        queue.enqueue("event-2", "second report")

        assertEquals(2, queue.pendingCount)
        assertEquals(listOf("first report", "second report"), queue.drain())
        assertEquals(0, queue.pendingCount)
    }

    @Test
    fun pendingReportsRemainQueuedUntilExplicitDrain() {
        val queue = BackgroundConversationUpdateQueue()
        queue.enqueue("event-1", "worker result")

        assertEquals(1, queue.pendingCount)
    }
}
