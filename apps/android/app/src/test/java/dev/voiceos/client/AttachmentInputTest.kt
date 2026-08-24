package dev.voiceos.client

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class AttachmentInputTest {
    @Test
    fun onlySupportedCameraImageTypesAreAccepted() {
        assertEquals("image/jpeg", AttachmentInput.acceptedMediaType("photo.JPG", "image/jpeg"))
        assertEquals("image/png", AttachmentInput.acceptedMediaType("receipt.png", null))
        assertEquals("image/webp", AttachmentInput.acceptedMediaType("scan.webp", "image/webp"))
        assertNull(AttachmentInput.acceptedMediaType("clip.gif", "image/gif"))
        assertNull(AttachmentInput.acceptedMediaType("notes.pdf", "application/pdf"))
    }
}
