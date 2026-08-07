package dev.voiceos.client

import org.json.JSONObject
import java.net.HttpURLConnection
import java.net.URL

internal object GatewayHttpTransport {
    fun <T> json(
        url: String,
        deviceToken: String?,
        request: ByteArray? = null,
        contentType: String = "application/json; charset=utf-8",
        decode: (JSONObject) -> T,
    ): T {
        val connection = URL(url).openConnection() as HttpURLConnection
        return try {
            configure(connection, deviceToken, request, contentType)
            decode(responseJson(connection))
        } finally {
            connection.disconnect()
        }
    }

    fun configure(
        connection: HttpURLConnection,
        deviceToken: String?,
        request: ByteArray? = null,
        contentType: String = "application/json; charset=utf-8",
    ) {
        connection.connectTimeout = 7_000
        connection.readTimeout = 90_000
        connection.setRequestProperty("Accept", "application/json")
        if (!deviceToken.isNullOrBlank()) {
            connection.setRequestProperty("Authorization", "Bearer $deviceToken")
        }
        if (request != null) {
            connection.requestMethod = "POST"
            connection.doOutput = true
            connection.setFixedLengthStreamingMode(request.size)
            connection.setRequestProperty("Content-Type", contentType)
            connection.outputStream.use { it.write(request) }
        }
    }

    fun responseJson(connection: HttpURLConnection): JSONObject {
        val status = connection.responseCode
        val stream = if (status in 200..299) connection.inputStream else connection.errorStream
        val body = stream?.bufferedReader()?.use { it.readText() }.orEmpty()
        if (status !in 200..299) {
            throw IllegalStateException("Gateway returned HTTP $status: $body")
        }
        return JSONObject(body)
    }
}
