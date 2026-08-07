package dev.voiceos.client

import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test
import java.io.ByteArrayOutputStream
import java.io.OutputStream
import java.net.HttpURLConnection
import java.net.URL

class GatewayHttpTransportTest {
    @Test
    fun configuresAuthenticatedJsonPostConsistently() {
        val connection = RecordingConnection()
        val body = "{\"value\":1}".toByteArray()

        GatewayHttpTransport.configure(connection, "device-token", body)

        assertEquals(7_000, connection.connectTimeout)
        assertEquals(90_000, connection.readTimeout)
        assertEquals("POST", connection.requestMethod)
        assertEquals("application/json", connection.getRequestProperty("Accept"))
        assertEquals("Bearer device-token", connection.getRequestProperty("Authorization"))
        assertEquals("application/json; charset=utf-8", connection.getRequestProperty("Content-Type"))
        assertArrayEquals(body, connection.output.toByteArray())
    }

    @Test
    fun leavesAuthorizationUnsetWhenNoDeviceTokenExists() {
        val connection = RecordingConnection()
        GatewayHttpTransport.configure(connection, null)
        assertEquals("GET", connection.requestMethod)
        assertNull(connection.getRequestProperty("Authorization"))
    }
}

private class RecordingConnection : HttpURLConnection(URL("http://voiceos.invalid")) {
    val output = ByteArrayOutputStream()

    override fun connect() = Unit
    override fun disconnect() = Unit
    override fun usingProxy() = false
    override fun getOutputStream(): OutputStream = output
}
