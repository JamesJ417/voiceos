package dev.voiceos.client

import org.json.JSONObject
import java.io.IOException
import java.net.HttpURLConnection
import java.net.URLEncoder
import java.net.URL
import java.util.concurrent.atomic.AtomicBoolean

data class TurnResult(
    val sessionId: String,
    val transcript: String,
    val responseText: String,
    val provider: String,
    val processingMs: Long,
    val approval: ApprovalRequest?,
)

data class ApprovalRequest(
    val requestId: String,
    val tool: String,
    val expiresAtUnix: Long,
    val arguments: JSONObject = JSONObject(),
)

data class ApprovalDecisionResult(
    val requestId: String,
    val status: String,
    val responseText: String,
)

data class GatewayHealth(
    val status: String,
    val gateway: String,
    val languageModel: String,
)

data class AuditTurn(
    val transcript: String,
    val responseText: String,
    val provider: String,
    val processingMs: Long,
    val createdAt: String,
)

data class EnrollmentResult(val deviceId: String, val deviceToken: String)

data class DocumentUploadResult(
    val documentId: String,
    val filename: String,
    val mode: String,
    val chunkCount: Int,
)

data class SkillProposal(
    val id: String,
    val name: String,
    val version: Int,
    val status: String,
    val content: String,
    val requiredCapabilities: List<String>,
    val evidenceJson: String,
    val evidenceCount: Int,
)

data class VoiceTask(
    val id: String,
    val title: String,
    val observableOutcome: String,
    val estimatedMinutes: Int,
    val status: String,
    val updatedAt: String,
    val vicInitiativeStatus: String = "",
    val vicSummary: String = "",
    val vicCapabilities: List<String> = emptyList(),
)

data class ClientEvent(val id: Long, val type: String, val payload: JSONObject)

class EventSubscription internal constructor(
    private val active: AtomicBoolean,
    private val connection: HttpURLConnection,
) {
    fun close() {
        active.set(false)
        connection.disconnect()
    }
}

object GatewayClient {
    fun streamEvents(
        baseUrl: String,
        deviceToken: String,
        after: Long,
        onEvent: (ClientEvent) -> Unit,
        onClosed: (Throwable?) -> Unit,
    ): EventSubscription {
        val connection = URL("$baseUrl/v1/events?after=${after.coerceAtLeast(0)}")
            .openConnection() as HttpURLConnection
        val active = AtomicBoolean(true)
        connection.connectTimeout = 7_000
        connection.readTimeout = 0
        connection.setRequestProperty("Accept", "text/event-stream")
        connection.setRequestProperty("Authorization", "Bearer $deviceToken")
        Thread({
            var failure: Throwable? = null
            try {
                if (connection.responseCode !in 200..299) {
                    throw IOException("Event stream returned HTTP ${connection.responseCode}")
                }
                connection.inputStream.bufferedReader().use { reader ->
                    var data: String? = null
                    while (active.get()) {
                        val line = reader.readLine() ?: break
                        when {
                            line.startsWith("data:") -> data = line.removePrefix("data:").trim()
                            line.isEmpty() && data != null -> {
                                val event = JSONObject(data)
                                onEvent(
                                    ClientEvent(
                                        event.getLong("id"),
                                        event.getString("type"),
                                        event.optJSONObject("payload") ?: JSONObject(),
                                    )
                                )
                                data = null
                            }
                        }
                    }
                }
            } catch (error: Throwable) {
                if (active.get()) failure = error
            } finally {
                connection.disconnect()
                onClosed(failure)
            }
        }, "voiceos-shared-events").start()
        return EventSubscription(active, connection)
    }

    fun getTasks(
        baseUrl: String,
        deviceToken: String?,
        limit: Int = 50,
        callback: (Result<List<VoiceTask>>) -> Unit,
    ) {
        Thread({
            callback(runCatching {
                retryConnection { getTasksBlocking(baseUrl, deviceToken, limit) }
            })
        }, "voiceos-task-list").start()
    }

    fun createTask(
        baseUrl: String,
        title: String,
        observableOutcome: String,
        estimatedMinutes: Int,
        deviceToken: String?,
        callback: (Result<VoiceTask>) -> Unit,
    ) {
        Thread({
            callback(runCatching {
                retryConnection {
                    createTaskBlocking(
                        baseUrl,
                        title,
                        observableOutcome,
                        estimatedMinutes,
                        deviceToken,
                    )
                }
            })
        }, "voiceos-task-create").start()
    }

    fun updateTaskStatus(
        baseUrl: String,
        taskId: String,
        status: String,
        deviceToken: String?,
        callback: (Result<VoiceTask>) -> Unit,
    ) {
        Thread({
            callback(runCatching {
                retryConnection { updateTaskStatusBlocking(baseUrl, taskId, status, deviceToken) }
            })
        }, "voiceos-task-status").start()
    }

    fun submitText(
        baseUrl: String,
        sessionId: String,
        text: String,
        deviceToken: String?,
        callback: (Result<TurnResult>) -> Unit,
    ) {
        Thread({
            callback(runCatching {
                retryConnection { submitTextBlocking(baseUrl, sessionId, text, deviceToken) }
            })
        }, "voiceos-text-turn").start()
    }

    fun getHealth(
        baseUrl: String,
        deviceToken: String?,
        callback: (Result<GatewayHealth>) -> Unit,
    ) {
        Thread({
            callback(runCatching {
                retryConnection { getHealthBlocking(baseUrl, deviceToken) }
            })
        }, "voiceos-health-check").start()
    }

    fun getHistory(
        baseUrl: String,
        deviceToken: String?,
        limit: Int = 30,
        callback: (Result<List<AuditTurn>>) -> Unit,
    ) {
        Thread({
            callback(runCatching {
                retryConnection { getHistoryBlocking(baseUrl, deviceToken, limit) }
            })
        }, "voiceos-history").start()
    }

    fun enroll(
        baseUrl: String,
        code: String,
        deviceName: String,
        callback: (Result<EnrollmentResult>) -> Unit,
    ) {
        Thread({
            callback(runCatching {
                retryConnection { enrollBlocking(baseUrl, code, deviceName) }
            })
        }, "voiceos-device-enrollment").start()
    }

    fun decideApproval(
        baseUrl: String,
        requestId: String,
        approve: Boolean,
        deviceToken: String?,
        callback: (Result<ApprovalDecisionResult>) -> Unit,
    ) {
        Thread({
            callback(runCatching {
                retryConnection {
                    decideApprovalBlocking(baseUrl, requestId, approve, deviceToken)
                }
            })
        }, "voiceos-approval-decision").start()
    }

    fun uploadDocument(
        baseUrl: String,
        filename: String,
        mediaType: String,
        mode: String,
        bytes: ByteArray,
        deviceToken: String?,
        callback: (Result<DocumentUploadResult>) -> Unit,
    ) {
        Thread({
            callback(runCatching {
                retryConnection {
                    uploadDocumentBlocking(
                        baseUrl,
                        filename,
                        mediaType,
                        mode,
                        bytes,
                        deviceToken,
                    )
                }
            })
        }, "voiceos-document-upload").start()
    }

    fun getSkillProposals(
        baseUrl: String,
        deviceToken: String?,
        callback: (Result<List<SkillProposal>>) -> Unit,
    ) {
        Thread({
            callback(runCatching {
                retryConnection { getSkillProposalsBlocking(baseUrl, deviceToken) }
            })
        }, "voiceos-skill-proposals").start()
    }

    fun decideSkillProposal(
        baseUrl: String,
        skillId: String,
        approve: Boolean,
        deviceToken: String?,
        callback: (Result<SkillProposal>) -> Unit,
    ) {
        Thread({
            callback(runCatching {
                retryConnection {
                    decideSkillProposalBlocking(baseUrl, skillId, approve, deviceToken)
                }
            })
        }, "voiceos-skill-decision").start()
    }

    private fun getHealthBlocking(baseUrl: String, deviceToken: String?): GatewayHealth {
        val connection = URL("$baseUrl/v1/health").openConnection() as HttpURLConnection
        try {
            configure(connection, deviceToken)
            val payload = responseJson(connection)
            return GatewayHealth(
                status = payload.getString("status"),
                gateway = payload.getString("gateway"),
                languageModel = payload.optString("language_model", "unknown"),
            )
        } finally {
            connection.disconnect()
        }
    }

    private fun getTasksBlocking(
        baseUrl: String,
        deviceToken: String?,
        limit: Int,
    ): List<VoiceTask> {
        val safeLimit = limit.coerceIn(1, 200)
        val connection = URL("$baseUrl/v1/tasks?limit=$safeLimit").openConnection() as HttpURLConnection
        try {
            configure(connection, deviceToken)
            val tasks = responseJson(connection).getJSONArray("tasks")
            return buildList {
                for (index in 0 until tasks.length()) {
                    add(parseTask(tasks.getJSONObject(index)))
                }
            }
        } finally {
            connection.disconnect()
        }
    }

    private fun createTaskBlocking(
        baseUrl: String,
        title: String,
        observableOutcome: String,
        estimatedMinutes: Int,
        deviceToken: String?,
    ): VoiceTask {
        val request = JSONObject()
            .put("title", title.trim())
            .put("observable_outcome", observableOutcome.trim())
            .put("estimated_minutes", estimatedMinutes.coerceIn(1, 1440))
            .toString()
            .toByteArray(Charsets.UTF_8)
        val connection = URL("$baseUrl/v1/tasks").openConnection() as HttpURLConnection
        try {
            configure(connection, deviceToken, request)
            val response = responseJson(connection)
            return parseTask(
                response.getJSONObject("task"),
                response.optJSONObject("initiative"),
            )
        } finally {
            connection.disconnect()
        }
    }

    private fun updateTaskStatusBlocking(
        baseUrl: String,
        taskId: String,
        status: String,
        deviceToken: String?,
    ): VoiceTask {
        val encodedId = URLEncoder.encode(taskId, Charsets.UTF_8.name())
        val request = JSONObject()
            .put("status", status)
            .toString()
            .toByteArray(Charsets.UTF_8)
        val connection = URL("$baseUrl/v1/tasks/$encodedId/status")
            .openConnection() as HttpURLConnection
        try {
            configure(connection, deviceToken, request)
            return parseTask(responseJson(connection).getJSONObject("task"))
        } finally {
            connection.disconnect()
        }
    }

    private fun parseTask(payload: JSONObject, initiative: JSONObject? = null): VoiceTask = VoiceTask(
        id = payload.getString("id"),
        title = payload.getString("title"),
        observableOutcome = payload.optString("observable_outcome"),
        estimatedMinutes = payload.optInt("estimated_minutes", 20),
        status = payload.optString("status", "ready"),
        updatedAt = payload.optString("updated_at"),
        vicInitiativeStatus = initiative?.optString("status").orEmpty(),
        vicSummary = initiative?.optString("summary").orEmpty(),
        vicCapabilities = buildList {
            val values = initiative?.optJSONArray("capabilities") ?: return@buildList
            for (index in 0 until values.length()) add(values.optString(index))
        },
    )

    private fun getHistoryBlocking(
        baseUrl: String,
        deviceToken: String?,
        limit: Int,
    ): List<AuditTurn> {
        val safeLimit = limit.coerceIn(1, 100)
        val connection = URL("$baseUrl/v1/audit/turns?limit=$safeLimit").openConnection() as HttpURLConnection
        try {
            configure(connection, deviceToken)
            val turns = responseJson(connection).getJSONArray("turns")
            return buildList {
                for (index in 0 until turns.length()) {
                    val turn = turns.getJSONObject(index)
                    add(
                        AuditTurn(
                            transcript = turn.optString("transcript"),
                            responseText = turn.optString("response_text"),
                            provider = turn.optString("provider", "unknown"),
                            processingMs = turn.optLong("processing_ms", 0),
                            createdAt = turn.optString("created_at"),
                        ),
                    )
                }
            }
        } finally {
            connection.disconnect()
        }
    }

    private fun submitTextBlocking(
        baseUrl: String,
        sessionId: String,
        text: String,
        deviceToken: String?,
    ): TurnResult {
        val request = JSONObject()
            .put("session_id", sessionId)
            .put("text", text)
            .toString()
            .toByteArray(Charsets.UTF_8)
        val connection = URL("$baseUrl/v1/turns/text").openConnection() as HttpURLConnection
        try {
            configure(connection, deviceToken, request)
            val payload = responseJson(connection)
            val approvals = payload.optJSONArray("approvals")
            val approval = approvals?.optJSONObject(0)?.let {
                ApprovalRequest(
                    requestId = it.getString("request_id"),
                    tool = it.getString("tool"),
                    expiresAtUnix = it.optLong("expires_at_unix", 0),
                    arguments = it.optJSONObject("arguments") ?: JSONObject(),
                )
            }
            return TurnResult(
                sessionId = payload.getString("session_id"),
                transcript = payload.getString("transcript"),
                responseText = payload.getString("response_text"),
                provider = payload.optString("provider", "unknown"),
                processingMs = payload.optLong("processing_ms", 0),
                approval = approval,
            )
        } finally {
            connection.disconnect()
        }
    }

    private fun decideApprovalBlocking(
        baseUrl: String,
        requestId: String,
        approve: Boolean,
        deviceToken: String?,
    ): ApprovalDecisionResult {
        val request = JSONObject()
            .put("request_id", requestId)
            .put("decision", if (approve) "approve" else "deny")
            .toString()
            .toByteArray(Charsets.UTF_8)
        val connection = URL("$baseUrl/v1/approvals/decide")
            .openConnection() as HttpURLConnection
        try {
            configure(connection, deviceToken, request)
            val payload = responseJson(connection)
            return ApprovalDecisionResult(
                requestId = payload.getString("request_id"),
                status = payload.getString("status"),
                responseText = payload.getString("response_text"),
            )
        } finally {
            connection.disconnect()
        }
    }

    private fun enrollBlocking(baseUrl: String, code: String, deviceName: String): EnrollmentResult {
        val request = JSONObject()
            .put("code", code)
            .put("device_name", deviceName)
            .toString()
            .toByteArray(Charsets.UTF_8)
        val connection = URL("$baseUrl/v1/enrollment/exchange")
            .openConnection() as HttpURLConnection
        try {
            configure(connection, null, request)
            val payload = responseJson(connection)
            return EnrollmentResult(
                deviceId = payload.getString("device_id"),
                deviceToken = payload.getString("device_token"),
            )
        } finally {
            connection.disconnect()
        }
    }

    private fun uploadDocumentBlocking(
        baseUrl: String,
        filename: String,
        mediaType: String,
        mode: String,
        bytes: ByteArray,
        deviceToken: String?,
    ): DocumentUploadResult {
        val encodedFilename = URLEncoder.encode(filename, Charsets.UTF_8.name())
            .replace("+", "%20")
        val connection = URL("$baseUrl/v1/files").openConnection() as HttpURLConnection
        try {
            connection.setRequestProperty("X-VoiceOS-File-Name", encodedFilename)
            connection.setRequestProperty("X-VoiceOS-Document-Mode", mode)
            configure(connection, deviceToken, bytes, mediaType)
            val document = responseJson(connection).getJSONObject("document")
            return DocumentUploadResult(
                documentId = document.getString("id"),
                filename = document.getString("filename"),
                mode = document.getString("mode"),
                chunkCount = document.optInt("chunk_count", 0),
            )
        } finally {
            connection.disconnect()
        }
    }

    private fun getSkillProposalsBlocking(
        baseUrl: String,
        deviceToken: String?,
    ): List<SkillProposal> {
        val connection = URL("$baseUrl/v1/skills/proposals?status=proposed&limit=20")
            .openConnection() as HttpURLConnection
        try {
            configure(connection, deviceToken)
            val proposals = responseJson(connection).getJSONArray("proposals")
            return buildList {
                for (index in 0 until proposals.length()) {
                    add(parseSkillProposal(proposals.getJSONObject(index)))
                }
            }
        } finally {
            connection.disconnect()
        }
    }

    private fun decideSkillProposalBlocking(
        baseUrl: String,
        skillId: String,
        approve: Boolean,
        deviceToken: String?,
    ): SkillProposal {
        val encodedId = URLEncoder.encode(skillId, Charsets.UTF_8.name())
        val request = JSONObject()
            .put("decision", if (approve) "approve" else "reject")
            .toString()
            .toByteArray(Charsets.UTF_8)
        val connection = URL("$baseUrl/v1/skills/proposals/$encodedId/decision")
            .openConnection() as HttpURLConnection
        try {
            configure(connection, deviceToken, request)
            return parseSkillProposal(responseJson(connection).getJSONObject("proposal"))
        } finally {
            connection.disconnect()
        }
    }

    private fun parseSkillProposal(payload: JSONObject): SkillProposal {
        val capabilities = payload.optJSONArray("required_capabilities")
        val evidence = payload.optJSONArray("evidence")
        return SkillProposal(
            id = payload.getString("id"),
            name = payload.getString("name"),
            version = payload.optInt("version", 1),
            status = payload.optString("status", "proposed"),
            content = payload.optString("content"),
            requiredCapabilities = buildList {
                if (capabilities != null) {
                    for (index in 0 until capabilities.length()) {
                        add(capabilities.optString(index))
                    }
                }
            },
            evidenceJson = evidence?.toString(2) ?: "[]",
            evidenceCount = evidence?.length() ?: 0,
        )
    }

    private fun configure(
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

    private fun responseJson(connection: HttpURLConnection): JSONObject {
        val status = connection.responseCode
        val stream = if (status in 200..299) connection.inputStream else connection.errorStream
        val body = stream?.bufferedReader()?.use { it.readText() }.orEmpty()
        if (status !in 200..299) {
            throw IllegalStateException("Gateway returned HTTP $status: $body")
        }
        return JSONObject(body)
    }

    private fun <T> retryConnection(action: () -> T): T {
        var lastError: IOException? = null
        repeat(3) { attempt ->
            try {
                return action()
            } catch (error: IOException) {
                lastError = error
                if (attempt < 2) Thread.sleep(500L shl attempt)
            }
        }
        throw lastError ?: IOException("Gateway connection failed")
    }
}
