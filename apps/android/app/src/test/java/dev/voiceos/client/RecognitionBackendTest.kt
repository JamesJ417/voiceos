package dev.voiceos.client

import org.junit.Assert.assertEquals
import org.junit.Test

class RecognitionBackendTest {
    @Test
    fun aStalledOnDeviceRecognizerFallsBackToThePlatformRecognizer() {
        assertEquals(RecognitionBackend.PLATFORM, RecognitionBackend.ON_DEVICE.afterStall())
        assertEquals(RecognitionBackend.PLATFORM, RecognitionBackend.PLATFORM.afterStall())
    }

    @Test
    fun aProvenBackendCanBeRestoredAcrossServiceRestarts() {
        assertEquals(
            RecognitionBackend.PLATFORM,
            RecognitionBackend.fromPersisted("platform"),
        )
        assertEquals(RecognitionBackend.PLATFORM, RecognitionBackend.fromPersisted(null))
    }
}
