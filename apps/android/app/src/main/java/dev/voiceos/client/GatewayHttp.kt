package dev.voiceos.client

import org.json.JSONObject
import java.net.HttpURLConnection

internal object GatewayTimeoutPolicy {
    const val DEFAULT_READ_MILLIS = 90_000
    const val LONG_TURN_READ_MILLIS = 420_000
}

class GatewayHttpException(
    val status: Int,
    val responseBody: String,
) : IllegalStateException("Gateway returned HTTP $status")

internal object GatewayHttp {
    fun configure(
        connection: HttpURLConnection,
        deviceToken: String?,
        request: ByteArray? = null,
        contentType: String = "application/json; charset=utf-8",
        readTimeoutMillis: Int = GatewayTimeoutPolicy.DEFAULT_READ_MILLIS,
        requestProperties: Map<String, String> = emptyMap(),
    ) {
        connection.connectTimeout = 7_000
        connection.readTimeout = readTimeoutMillis
        connection.setRequestProperty("Accept", "application/json")
        if (!deviceToken.isNullOrBlank()) {
            connection.setRequestProperty("Authorization", "Bearer $deviceToken")
        }
        requestProperties.forEach { (name, value) ->
            connection.setRequestProperty(name, value)
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
            throw GatewayHttpException(status, body)
        }
        return JSONObject(body)
    }
}
