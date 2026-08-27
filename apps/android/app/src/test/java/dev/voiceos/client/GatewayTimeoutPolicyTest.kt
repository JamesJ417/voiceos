package dev.voiceos.client

import java.io.ByteArrayOutputStream
import java.io.IOException
import java.net.ConnectException
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
        assertEquals(
            GatewayTimeoutPolicy.LONG_TURN_READ_MILLIS.toLong(),
            GatewayTimeoutPolicy.CONVERSATION_TURN_WATCHDOG_MILLIS,
        )
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

    @Test
    fun retriesOnlyFailuresThatCannotHaveReachedTheGateway() {
        assertTrue(GatewayTransportPolicy.canRetryWithoutDuplicatingRequest(ConnectException()))
        assertTrue(!GatewayTransportPolicy.canRetryWithoutDuplicatingRequest(IOException("reset")))
        assertEquals(150L, GatewayTransportPolicy.retryDelayMillis(0))
        assertEquals(500L, GatewayTransportPolicy.reconnectDelayMillis(1))
        assertEquals(30_000L, GatewayTransportPolicy.reconnectDelayMillis(20))
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
