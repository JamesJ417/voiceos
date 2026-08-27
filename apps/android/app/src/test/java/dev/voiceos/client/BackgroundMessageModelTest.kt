package dev.voiceos.client

import org.junit.Assert.assertEquals
import org.junit.Test

class BackgroundMessageModelTest {
    @Test fun messageValuePreservesStableEventIdAndReadState() {
        val unread = BackgroundMessage("event-1", "VIC finished the report")
        assertEquals("event-1", unread.id)
        assertEquals("VIC finished the report", unread.text)
        assertEquals(false, unread.read)
        assertEquals(true, unread.copy(read = true).read)
    }
}
