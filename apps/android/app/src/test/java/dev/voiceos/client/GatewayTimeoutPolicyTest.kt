package dev.voiceos.client

import java.io.ByteArrayOutputStream
import java.net.HttpURLConnection
import java.net.URL
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class GatewayTimeoutPolicyTest {
    @Test
    fun longTurnsOutliveTheProviderDeadlineAndOrdinaryCallsStayBounded() {
        assertEquals(90_000, GatewayTimeoutPolicy.DEFAULT_READ_MILLIS)
        assertEquals(420_000, GatewayTimeoutPolicy.LONG_TURN_READ_MILLIS)
        assertTrue(GatewayTimeoutPolicy.LONG_TURN_READ_MILLIS > 360_000)
    }

    @Test
    fun requestPropertiesAreAppliedBeforeWritingTheRequestBody() {
        val connection = RecordingConnection()
        val request = "{\"text\":\"hello\"}".toByteArray()

        GatewayHttp.configure(
            connection = connection,
            deviceToken = "device-token",
            request = request,
            requestProperties = mapOf("Idempotency-Key" to "turn-123"),
        )

        assertEquals("turn-123", connection.recordedProperties["Idempotency-Key"])
        assertEquals(request.toList(), connection.writtenBytes.toByteArray().toList())
    }

    private class RecordingConnection : HttpURLConnection(URL("http://voiceos.test")) {
        val writtenBytes = ByteArrayOutputStream()
        val recordedProperties = mutableMapOf<String, String>()

        override fun setRequestProperty(key: String, value: String) {
            check(!connected) { "Cannot set request property after connection is made" }
            recordedProperties[key] = value
        }

        override fun getOutputStream(): ByteArrayOutputStream {
            connected = true
            return writtenBytes
        }

        override fun disconnect() = Unit

        override fun usingProxy(): Boolean = false

        override fun connect() {
            connected = true
        }
    }
}
