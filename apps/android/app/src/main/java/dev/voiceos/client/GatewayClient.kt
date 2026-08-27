package dev.voiceos.client

import org.json.JSONObject
import java.io.IOException
import java.net.HttpURLConnection
import java.net.URLEncoder
import java.net.URL
import java.security.MessageDigest
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
    val memory: String,
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

data class AttachmentUploadResult(
    val id: String,
    val filename: String,
    val mediaType: String,
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

data class SkillUsage(
    val id: String,
    val skillId: String,
    val skillName: String,
    val skillVersion: Int,
    val outcome: String,
    val feedback: String?,
    val usedAt: String,
)

data class VoiceTask(
    val id: String,
    val projectId: String? = null,
    val parentTaskId: String? = null,
    val title: String,
    val observableOutcome: String,
    val estimatedMinutes: Int,
    val dueAt: String? = null,
    val importance: String = "normal",
    val status: String,
    val updatedAt: String,
    val vicInitiativeStatus: String = "",
    val vicSummary: String = "",
    val vicCapabilities: List<String> = emptyList(),
    val progressLane: String = "shared",
    val vicStatus: String = "not_analyzed",
    val completedSteps: Int = 0,
    val totalSteps: Int = 0,
    val openBlockers: Int = 0,
    val nextUserAction: String = "",
    val nextVicAction: String = "",
    val steps: List<VoiceTaskStep> = emptyList(),
    val handoffs: List<VoiceTaskHandoff> = emptyList(),
    val artifacts: List<VoiceTaskArtifact> = emptyList(),
)

data class VoiceTaskStep(
    val title: String,
    val owner: String,
    val status: String,
    val id: String = "",
    val position: Int = 0,
    val updatedAt: String = "",
)
data class VoiceTaskHandoff(
    val id: String,
    val fromOwner: String,
    val toOwner: String,
    val kind: String,
    val summary: String,
    val status: String,
    val createdAt: String,
    val completedAt: String? = null,
)
data class VoiceTaskArtifact(val kind: String, val uri: String, val description: String)

data class VoiceProject(
    val id: String,
    val title: String,
    val status: String,
    val updatedAt: String,
)

data class BridgeNotification(
    val id: Long,
    val payload: JSONObject,
    val createdAt: String,
)

data class ClientEvent(val id: Long, val type: String, val payload: JSONObject, val createdAt: String? = null)

data class EventRecovery(
    val latestEventId: Long,
    val events: List<ClientEvent>,
)

data class ConversationFloor(
    val conversationId: String,
    val leaseId: String?,
    val holderDeviceId: String?,
    val holderDisplayName: String?,
    val phase: String,
    val partialTranscript: String?,
    val responseText: String?,
    val revision: Long,
    val expiresAtUnix: Long,
    val active: Boolean,
)

data class ConversationFloorRequest(
    val action: String,
    val phase: String,
    val partialTranscript: String? = null,
    val responseText: String? = null,
    val expectedLeaseId: String? = null,
    val expectedRevision: Long? = null,
)

data class VicOutreach(
    val id: String,
    val kind: String,
    val priority: String,
    val title: String,
    val body: String,
    val reason: String,
    val status: String,
    val taskId: String?,
)

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
    fun getRecentEvents(
        baseUrl: String,
        deviceToken: String?,
        callback: (Result<EventRecovery>) -> Unit,
    ) {
        Thread({
            callback(runCatching {
                retryConnection { getRecentEventsBlocking(baseUrl, deviceToken) }
            })
        }, "voiceos-event-recovery").start()
    }

    fun getLatestEventCursor(
        baseUrl: String,
        deviceToken: String?,
        callback: (Result<Long>) -> Unit,
    ) {
        Thread({
            callback(runCatching {
                retryConnection { getLatestEventCursorBlocking(baseUrl, deviceToken) }
            })
        }, "voiceos-event-cursor").start()
    }

    fun changeConversationFloor(
        baseUrl: String,
        request: ConversationFloorRequest,
        deviceToken: String?,
        callback: (Result<ConversationFloor>) -> Unit = {},
    ) {
        Thread({
            callback(runCatching {
                retryConnection { changeConversationFloorBlocking(baseUrl, request, deviceToken) }
            })
        }, "voiceos-conversation-floor").start()
    }

    fun createTestOutreach(
        baseUrl: String,
        deviceToken: String?,
        callback: (Result<VicOutreach>) -> Unit,
    ) {
        Thread({ callback(runCatching { createTestOutreachBlocking(baseUrl, deviceToken) }) }, "vic-test-outreach").start()
    }

    fun getPendingOutreach(
        baseUrl: String,
        deviceToken: String?,
        callback: (Result<List<VicOutreach>>) -> Unit,
    ) {
        Thread({ callback(runCatching { getPendingOutreachBlocking(baseUrl, deviceToken) }) }, "vic-outreach-recovery").start()
    }

    fun getBridgeInbox(
        baseUrl: String,
        deviceToken: String?,
        callback: (Result<List<BridgeNotification>>) -> Unit,
    ) {
        Thread({ callback(runCatching { getBridgeInboxBlocking(baseUrl, deviceToken) }) }, "voiceos-bridge-inbox").start()
    }

    fun actOnOutreach(
        baseUrl: String,
        deviceToken: String?,
        outreachId: String,
        action: String,
        snoozeMinutes: Int? = null,
        callback: (Result<VicOutreach>) -> Unit = {},
    ) {
        Thread({ callback(runCatching { actOnOutreachBlocking(baseUrl, deviceToken, outreachId, action, snoozeMinutes) }) }, "vic-outreach-action").start()
    }

    fun streamEvents(
        baseUrl: String,
        deviceToken: String,
        after: Long,
        onEvent: (ClientEvent) -> Unit,
        onClosed: (Throwable?) -> Unit,
        onConnected: (Long) -> Unit = {},
    ): EventSubscription {
        val connection = URL("$baseUrl/v1/events?after=${after.coerceAtLeast(0)}")
            .openConnection() as HttpURLConnection
        val active = AtomicBoolean(true)
        connection.connectTimeout = 7_000
        connection.readTimeout = 0
        connection.setRequestProperty("Accept", "text/event-stream")
        connection.setRequestProperty("Cache-Control", "no-cache")
        connection.setRequestProperty("Authorization", "Bearer $deviceToken")
        if (after > 0) connection.setRequestProperty("Last-Event-ID", after.toString())
        Thread({
            var failure: Throwable? = null
            val startedAt = System.nanoTime()
            try {
                if (connection.responseCode !in 200..299) {
                    throw IOException("Event stream returned HTTP ${connection.responseCode}")
                }
                onConnected((System.nanoTime() - startedAt) / 1_000_000)
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
                                        event.optString("created_at").takeIf { it.isNotBlank() },
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

    fun getProjects(
        baseUrl: String,
        deviceToken: String?,
        callback: (Result<List<VoiceProject>>) -> Unit,
    ) {
        Thread({
            callback(runCatching {
                retryConnection { getProjectsBlocking(baseUrl, deviceToken) }
            })
        }, "voiceos-project-list").start()
    }

    fun createProject(
        baseUrl: String,
        title: String,
        deviceToken: String?,
        callback: (Result<VoiceProject>) -> Unit,
    ) {
        Thread({
            callback(runCatching {
                retryConnection { createProjectBlocking(baseUrl, title, deviceToken) }
            })
        }, "voiceos-project-create").start()
    }

    fun createTask(
        baseUrl: String,
        title: String,
        observableOutcome: String,
        estimatedMinutes: Int,
        projectId: String?,
        dueAt: String?,
        importance: String,
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
                        projectId,
                        dueAt,
                        importance,
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

    fun recordTaskProgress(
        baseUrl: String,
        taskId: String,
        summary: String,
        deviceToken: String?,
        callback: (Result<Unit>) -> Unit,
    ) {
        Thread({
            callback(runCatching {
                retryConnection { recordTaskProgressBlocking(baseUrl, taskId, summary, deviceToken) }
            })
        }, "voiceos-task-progress").start()
    }

    fun updateTaskStep(
        baseUrl: String,
        taskId: String,
        stepId: String,
        status: String,
        deviceToken: String?,
        callback: (Result<VoiceTask>) -> Unit,
    ) {
        Thread({
            callback(runCatching {
                retryConnection {
                    taskActionBlocking(
                        baseUrl,
                        taskId,
                        JSONObject()
                            .put("action", "step.update")
                            .put("step_id", stepId)
                            .put("status", status)
                            .put("evidence", JSONObject().put("source", "voiceos_android")),
                        deviceToken,
                    )
                }
            })
        }, "voiceos-stage-update").start()
    }

    fun advanceTaskStep(
        baseUrl: String,
        taskId: String,
        stepId: String,
        summary: String,
        deviceToken: String?,
        callback: (Result<VoiceTask>) -> Unit,
    ) {
        Thread({
            callback(runCatching {
                retryConnection {
                    taskActionBlocking(
                        baseUrl,
                        taskId,
                        JSONObject()
                            .put("action", "step.advance")
                            .put("step_id", stepId)
                            .put("summary", summary)
                            .put("evidence", JSONObject().put("source", "voiceos_android")),
                        deviceToken,
                    )
                }
            })
        }, "voiceos-stage-advance").start()
    }

    fun updateTaskHandoff(
        baseUrl: String,
        taskId: String,
        handoffId: String,
        status: String,
        deviceToken: String?,
        callback: (Result<VoiceTask>) -> Unit,
    ) {
        Thread({
            callback(runCatching {
                retryConnection {
                    taskActionBlocking(
                        baseUrl,
                        taskId,
                        JSONObject()
                            .put("action", "handoff.update")
                            .put("handoff_id", handoffId)
                            .put("status", status),
                        deviceToken,
                    )
                }
            })
        }, "voiceos-handoff-update").start()
    }

    fun setTaskAttention(
        baseUrl: String,
        taskId: String,
        importance: String,
        dueAt: String?,
        deviceToken: String?,
        callback: (Result<VoiceTask>) -> Unit,
    ) {
        Thread({
            callback(runCatching {
                retryConnection { setTaskAttentionBlocking(baseUrl, taskId, importance, dueAt, deviceToken) }
            })
        }, "voiceos-task-attention").start()
    }

    fun submitText(
        baseUrl: String,
        sessionId: String,
        text: String,
        deviceToken: String?,
        attachmentIds: List<String> = emptyList(),
        requestId: String = java.util.UUID.randomUUID().toString(),
        callback: (Result<TurnResult>) -> Unit,
    ) {
        Thread({
            callback(runCatching {
                retryConnection { submitTextBlocking(baseUrl, sessionId, text, deviceToken, requestId, attachmentIds) }
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

    fun uploadAttachment(
        baseUrl: String,
        filename: String,
        mediaType: String,
        bytes: ByteArray,
        deviceToken: String?,
        callback: (Result<AttachmentUploadResult>) -> Unit,
    ) {
        Thread({
            callback(runCatching {
                retryConnection { uploadAttachmentBlocking(baseUrl, filename, mediaType, bytes, deviceToken) }
            })
        }, "voiceos-attachment-upload").start()
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

    fun getSkills(
        baseUrl: String,
        deviceToken: String?,
        callback: (Result<List<SkillProposal>>) -> Unit,
    ) {
        Thread({ callback(runCatching { retryConnection { getSkillsBlocking(baseUrl, deviceToken) } }) }, "voiceos-skills").start()
    }

    fun getSkillUsages(
        baseUrl: String,
        deviceToken: String?,
        callback: (Result<List<SkillUsage>>) -> Unit,
    ) {
        Thread({ callback(runCatching { retryConnection { getSkillUsagesBlocking(baseUrl, deviceToken) } }) }, "voiceos-skill-usages").start()
    }

    fun setSkillEnabled(
        baseUrl: String,
        skillId: String,
        enabled: Boolean,
        deviceToken: String?,
        callback: (Result<SkillProposal>) -> Unit,
    ) {
        Thread({ callback(runCatching { retryConnection { setSkillEnabledBlocking(baseUrl, skillId, enabled, deviceToken) } }) }, "voiceos-skill-status").start()
    }

    fun reviewSkillUsage(
        baseUrl: String,
        usageId: String,
        correct: Boolean,
        deviceToken: String?,
        callback: (Result<SkillUsage>) -> Unit,
    ) {
        Thread({ callback(runCatching { retryConnection { reviewSkillUsageBlocking(baseUrl, usageId, correct, deviceToken) } }) }, "voiceos-skill-feedback").start()
    }

    private fun getHealthBlocking(baseUrl: String, deviceToken: String?): GatewayHealth {
        val connection = URL("$baseUrl/v1/health").openConnection() as HttpURLConnection
        try {
            GatewayHttp.configure(connection, deviceToken)
            val payload = GatewayHttp.responseJson(connection)
            return GatewayHealth(
                status = payload.getString("status"),
                gateway = payload.getString("gateway"),
                languageModel = payload.optString("language_model", "unknown"),
                memory = payload.optString("memory", "unavailable"),
            )
        } finally {
            connection.disconnect()
        }
    }

    private fun getLatestEventCursorBlocking(baseUrl: String, deviceToken: String?): Long {
        val connection = URL("$baseUrl/v1/events/recovery?after=0&tail=true")
            .openConnection() as HttpURLConnection
        try {
            GatewayHttp.configure(connection, deviceToken)
            return GatewayHttp.responseJson(connection).optLong("latest_event_id", 0L).coerceAtLeast(0L)
        } finally {
            connection.disconnect()
        }
    }

    private fun getRecentEventsBlocking(baseUrl: String, deviceToken: String?): EventRecovery {
        val connection = URL("$baseUrl/v1/events/recovery?after=0&tail=true")
            .openConnection() as HttpURLConnection
        try {
            GatewayHttp.configure(connection, deviceToken)
            val response = GatewayHttp.responseJson(connection)
            val values = response.optJSONArray("events")
            val events = buildList {
                if (values != null) for (index in 0 until values.length()) {
                    val event = values.optJSONObject(index) ?: continue
                    add(
                        ClientEvent(
                            id = event.optLong("id", 0L),
                            type = event.optString("type"),
                            payload = event.optJSONObject("payload") ?: JSONObject(),
                            createdAt = event.optString("created_at").takeIf { it.isNotBlank() },
                        )
                    )
                }
            }
            return EventRecovery(
                latestEventId = response.optLong("latest_event_id", 0L).coerceAtLeast(0L),
                events = events,
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
        val connection = URL("$baseUrl/v1/tasks?include_completed=true&limit=$safeLimit").openConnection() as HttpURLConnection
        try {
            GatewayHttp.configure(connection, deviceToken)
            val response = GatewayHttp.responseJson(connection)
            val tasks = response.getJSONArray("tasks")
            val details = response.optJSONArray("details")
            val detailByTask = buildMap {
                if (details != null) for (index in 0 until details.length()) {
                    val detail = details.optJSONObject(index) ?: continue
                    val id = detail.optJSONObject("task")?.optString("id").orEmpty()
                    if (id.isNotBlank()) put(id, detail)
                }
            }
            return buildList {
                for (index in 0 until tasks.length()) {
                    val task = tasks.getJSONObject(index)
                    add(parseTask(task, detail = detailByTask[task.optString("id")]))
                }
            }
        } finally {
            connection.disconnect()
        }
    }

    private fun getProjectsBlocking(
        baseUrl: String,
        deviceToken: String?,
    ): List<VoiceProject> {
        val connection = URL("$baseUrl/v1/projects?limit=100").openConnection() as HttpURLConnection
        try {
            GatewayHttp.configure(connection, deviceToken)
            val projects = GatewayHttp.responseJson(connection).getJSONArray("projects")
            return buildList {
                for (index in 0 until projects.length()) {
                    val project = projects.getJSONObject(index)
                    add(parseProject(project))
                }
            }
        } finally {
            connection.disconnect()
        }
    }

    private fun createProjectBlocking(
        baseUrl: String,
        title: String,
        deviceToken: String?,
    ): VoiceProject {
        val request = JSONObject()
            .put("title", title.trim())
            .toString()
            .toByteArray(Charsets.UTF_8)
        val connection = URL("$baseUrl/v1/projects").openConnection() as HttpURLConnection
        try {
            GatewayHttp.configure(connection, deviceToken, request)
            return parseProject(GatewayHttp.responseJson(connection).getJSONObject("project"))
        } finally {
            connection.disconnect()
        }
    }

    private fun parseProject(payload: JSONObject) = VoiceProject(
        id = payload.getString("id"),
        title = payload.getString("title"),
        status = payload.optString("status", "active"),
        updatedAt = payload.optString("updated_at"),
    )

    private fun createTaskBlocking(
        baseUrl: String,
        title: String,
        observableOutcome: String,
        estimatedMinutes: Int,
        projectId: String?,
        dueAt: String?,
        importance: String,
        deviceToken: String?,
    ): VoiceTask {
        val request = JSONObject()
            .put("title", title.trim())
            .put("observable_outcome", observableOutcome.trim())
            .put("estimated_minutes", estimatedMinutes.coerceIn(1, 1440))
            .put("project_id", projectId ?: JSONObject.NULL)
            .put("due_at", dueAt ?: JSONObject.NULL)
            .put("importance", importance.takeIf { it in setOf("low", "normal", "high", "critical") } ?: "normal")
            .toString()
            .toByteArray(Charsets.UTF_8)
        val connection = URL("$baseUrl/v1/tasks").openConnection() as HttpURLConnection
        try {
            GatewayHttp.configure(connection, deviceToken, request)
            val response = GatewayHttp.responseJson(connection)
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
            GatewayHttp.configure(connection, deviceToken, request)
            return parseTask(GatewayHttp.responseJson(connection).getJSONObject("task"))
        } finally {
            connection.disconnect()
        }
    }

    private fun recordTaskProgressBlocking(
        baseUrl: String,
        taskId: String,
        summary: String,
        deviceToken: String?,
    ) {
        val encodedId = URLEncoder.encode(taskId, Charsets.UTF_8.name())
        val request = JSONObject()
            .put("action", "progress.record")
            .put("summary", summary.trim())
            .put("evidence", JSONObject().put("source", "ov_brain_dump"))
            .toString()
            .toByteArray(Charsets.UTF_8)
        val connection = URL("$baseUrl/v1/tasks/$encodedId/actions").openConnection() as HttpURLConnection
        try {
            GatewayHttp.configure(connection, deviceToken, request)
            GatewayHttp.responseJson(connection)
        } finally {
            connection.disconnect()
        }
    }

    private fun taskActionBlocking(
        baseUrl: String,
        taskId: String,
        request: JSONObject,
        deviceToken: String?,
    ): VoiceTask {
        val encodedId = URLEncoder.encode(taskId, Charsets.UTF_8.name())
        val connection = URL("$baseUrl/v1/tasks/$encodedId/actions").openConnection() as HttpURLConnection
        try {
            GatewayHttp.configure(connection, deviceToken, request.toString().toByteArray(Charsets.UTF_8))
            val detail = GatewayHttp.responseJson(connection).getJSONObject("detail")
            return parseTask(detail.getJSONObject("task"), detail = detail)
        } finally {
            connection.disconnect()
        }
    }

    private fun setTaskAttentionBlocking(
        baseUrl: String,
        taskId: String,
        importance: String,
        dueAt: String?,
        deviceToken: String?,
    ): VoiceTask {
        val encodedId = URLEncoder.encode(taskId, Charsets.UTF_8.name())
        val request = JSONObject()
            .put("importance", importance.takeIf { it in setOf("high", "normal", "low") } ?: "normal")
            .put("due_at", dueAt ?: JSONObject.NULL)
            .toString()
            .toByteArray(Charsets.UTF_8)
        val connection = URL("$baseUrl/v1/tasks/$encodedId/attention").openConnection() as HttpURLConnection
        try {
            GatewayHttp.configure(connection, deviceToken, request)
            return parseTask(GatewayHttp.responseJson(connection).getJSONObject("task"))
        } finally {
            connection.disconnect()
        }
    }

    private fun parseTask(
        payload: JSONObject,
        initiative: JSONObject? = null,
        detail: JSONObject? = null,
    ): VoiceTask {
        val progress = detail?.optJSONObject("progress")
        val detailInitiative = detail?.optJSONObject("initiative")
        val steps = detail?.optJSONArray("steps")
        val handoffs = detail?.optJSONArray("handoffs")
        val artifacts = detail?.optJSONArray("artifacts")
        return VoiceTask(
        id = payload.getString("id"),
        projectId = payload.optString("project_id").takeIf { it.isNotBlank() && it != "null" },
        parentTaskId = payload.optString("parent_task_id").takeIf { it.isNotBlank() && it != "null" },
        title = payload.getString("title"),
        observableOutcome = payload.optString("observable_outcome"),
        estimatedMinutes = payload.optInt("estimated_minutes", 20),
        dueAt = payload.optString("due_at").takeIf { it.isNotBlank() && it != "null" },
        importance = payload.optString("importance", "normal"),
        status = payload.optString("status", "ready"),
        updatedAt = payload.optString("updated_at"),
        vicInitiativeStatus = initiative?.optString("status")
            ?: detailInitiative?.optString("status").orEmpty(),
        vicSummary = initiative?.optString("summary").orEmpty(),
        vicCapabilities = buildList {
            val values = initiative?.optJSONArray("capabilities") ?: return@buildList
            for (index in 0 until values.length()) add(values.optString(index))
        },
        progressLane = progress?.optString("lane", "shared") ?: "shared",
        vicStatus = progress?.optString("vic_status", "not_analyzed") ?: "not_analyzed",
        completedSteps = progress?.optInt("completed_steps", 0) ?: 0,
        totalSteps = progress?.optInt("total_steps", 0) ?: 0,
        openBlockers = progress?.optInt("open_blockers", 0) ?: 0,
        nextUserAction = progress?.optString("next_user_action").orEmpty(),
        nextVicAction = progress?.optString("next_vic_action").orEmpty(),
        steps = buildList {
            if (steps != null) for (index in 0 until steps.length()) {
                val step = steps.optJSONObject(index) ?: continue
                add(VoiceTaskStep(
                    title = step.optString("title"),
                    owner = step.optString("owner"),
                    status = step.optString("status"),
                    id = step.optString("id"),
                    position = step.optInt("position", index),
                    updatedAt = step.optString("updated_at"),
                ))
            }
        },
        handoffs = buildList {
            if (handoffs != null) for (index in 0 until handoffs.length()) {
                val handoff = handoffs.optJSONObject(index) ?: continue
                add(VoiceTaskHandoff(
                    id = handoff.optString("id"),
                    fromOwner = handoff.optString("from_owner"),
                    toOwner = handoff.optString("to_owner"),
                    kind = handoff.optString("kind"),
                    summary = handoff.optString("summary"),
                    status = handoff.optString("status"),
                    createdAt = handoff.optString("created_at"),
                    completedAt = handoff.optString("completed_at").takeIf { it.isNotBlank() && it != "null" },
                ))
            }
        },
        artifacts = buildList {
            if (artifacts != null) for (index in 0 until artifacts.length()) {
                val artifact = artifacts.optJSONObject(index) ?: continue
                add(VoiceTaskArtifact(artifact.optString("kind"), artifact.optString("uri"), artifact.optString("description")))
            }
        },
    )
    }

    private fun getHistoryBlocking(
        baseUrl: String,
        deviceToken: String?,
        limit: Int,
    ): List<AuditTurn> {
        val safeLimit = limit.coerceIn(1, 100)
        val connection = URL("$baseUrl/v1/audit/turns?limit=$safeLimit").openConnection() as HttpURLConnection
        try {
            GatewayHttp.configure(connection, deviceToken)
            val turns = GatewayHttp.responseJson(connection).getJSONArray("turns")
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
        requestId: String,
        attachmentIds: List<String>,
    ): TurnResult {
        val attachments = org.json.JSONArray().apply {
            attachmentIds.forEach { attachmentId ->
                put(JSONObject().put("attachment_id", attachmentId).put("purpose", "input_image"))
            }
        }
        val request = JSONObject()
            .put("session_id", sessionId)
            .put("text", text)
            .put("request_id", requestId)
            // Keep the legacy field while gateways roll out the attachment objects.
            .put("attachment_ids", org.json.JSONArray(attachmentIds))
            .put("attachments", attachments)
            .toString()
            .toByteArray(Charsets.UTF_8)
        val connection = URL("$baseUrl/v1/turns/text").openConnection() as HttpURLConnection
        try {
            GatewayHttp.configure(
                connection,
                deviceToken,
                request,
                readTimeoutMillis = GatewayTimeoutPolicy.LONG_TURN_READ_MILLIS,
                requestProperties = mapOf("Idempotency-Key" to requestId),
            )
            val payload = GatewayHttp.responseJson(connection)
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

    private fun changeConversationFloorBlocking(
        baseUrl: String,
        floorRequest: ConversationFloorRequest,
        deviceToken: String?,
    ): ConversationFloor {
        val request = JSONObject()
            .put("action", floorRequest.action)
            .put("phase", floorRequest.phase)
            .put("partial_transcript", floorRequest.partialTranscript)
            .put("response_text", floorRequest.responseText)
            .put("display_name", "Pixel")
            .put("ttl_seconds", 45)
            .put("expected_lease_id", floorRequest.expectedLeaseId)
            .put("expected_revision", floorRequest.expectedRevision)
            .toString()
            .toByteArray(Charsets.UTF_8)
        val connection = URL("$baseUrl/v1/conversations/active/floor")
            .openConnection() as HttpURLConnection
        try {
            GatewayHttp.configure(connection, deviceToken, request)
            return parseConversationFloor(GatewayHttp.responseJson(connection).getJSONObject("floor"))
        } finally {
            connection.disconnect()
        }
    }

    fun parseConversationFloor(value: JSONObject): ConversationFloor = ConversationFloor(
        conversationId = value.optString("conversation_id"),
        leaseId = value.optString("lease_id").takeIf(String::isNotBlank),
        holderDeviceId = value.optString("holder_device_id").takeIf(String::isNotBlank),
        holderDisplayName = value.optString("holder_display_name").takeIf(String::isNotBlank),
        phase = value.optString("phase", "idle"),
        partialTranscript = value.optString("partial_transcript").takeIf(String::isNotBlank),
        responseText = value.optString("response_text").takeIf(String::isNotBlank),
        revision = value.optLong("revision", 0),
        expiresAtUnix = value.optLong("expires_at_unix", 0),
        active = value.optBoolean("active", false),
    )

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
            GatewayHttp.configure(connection, deviceToken, request)
            val payload = GatewayHttp.responseJson(connection)
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
            GatewayHttp.configure(connection, null, request)
            val payload = GatewayHttp.responseJson(connection)
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
            GatewayHttp.configure(connection, deviceToken, bytes, mediaType)
            val document = GatewayHttp.responseJson(connection).getJSONObject("document")
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

    private fun uploadAttachmentBlocking(
        baseUrl: String,
        filename: String,
        mediaType: String,
        bytes: ByteArray,
        deviceToken: String?,
    ): AttachmentUploadResult {
        AttachmentInput.requireUploadSize(bytes.size)
        val encodedFilename = URLEncoder.encode(filename, Charsets.UTF_8.name()).replace("+", "%20")
        val sha256 = MessageDigest.getInstance("SHA-256").digest(bytes)
            .joinToString("") { "%02x".format(it) }

        val create = URL("$baseUrl/v1/uploads").openConnection() as HttpURLConnection
        val upload = try {
            create.setRequestProperty("X-VoiceOS-File-Name", encodedFilename)
            create.setRequestProperty("X-VoiceOS-Upload-Length", bytes.size.toString())
            create.setRequestProperty("X-VoiceOS-Upload-SHA256", sha256)
            GatewayHttp.configure(create, deviceToken, null, mediaType)
            create.requestMethod = "POST"
            GatewayHttp.responseJson(create).getJSONObject("upload")
        } finally {
            create.disconnect()
        }

        val uploadId = upload.getString("upload_id")
        val chunkSize = upload.getInt("chunk_size")
        var offset = 0
        while (offset < bytes.size) {
            val end = minOf(offset + chunkSize, bytes.size)
            val chunk = bytes.copyOfRange(offset, end)
            val chunkConnection = URL("$baseUrl/v1/uploads/${URLEncoder.encode(uploadId, Charsets.UTF_8.name())}/chunks/$offset")
                .openConnection() as HttpURLConnection
            try {
                chunkConnection.requestMethod = "PUT"
                chunkConnection.doOutput = true
                chunkConnection.setRequestProperty("Authorization", "Bearer $deviceToken")
                chunkConnection.setRequestProperty("Accept", "application/json")
                chunkConnection.setRequestProperty("Content-Type", "application/octet-stream")
                chunkConnection.setFixedLengthStreamingMode(chunk.size)
                chunkConnection.outputStream.use { it.write(chunk) }
                GatewayHttp.responseJson(chunkConnection)
            } finally {
                chunkConnection.disconnect()
            }
            offset = end
        }

        val finalize = URL("$baseUrl/v1/uploads/${URLEncoder.encode(uploadId, Charsets.UTF_8.name())}/finalize")
            .openConnection() as HttpURLConnection
        try {
            GatewayHttp.configure(finalize, deviceToken)
            finalize.requestMethod = "POST"
            val attachment = GatewayHttp.responseJson(finalize).getJSONObject("attachment")
            return AttachmentUploadResult(
                id = attachment.getString("attachment_id"),
                filename = attachment.getString("filename"),
                mediaType = attachment.getString("media_type"),
            )
        } finally {
            finalize.disconnect()
        }
    }

    private fun getSkillProposalsBlocking(
        baseUrl: String,
        deviceToken: String?,
    ): List<SkillProposal> {
        val connection = URL("$baseUrl/v1/skills/proposals?status=proposed&limit=20")
            .openConnection() as HttpURLConnection
        try {
            GatewayHttp.configure(connection, deviceToken)
            val proposals = GatewayHttp.responseJson(connection).getJSONArray("proposals")
            return buildList {
                for (index in 0 until proposals.length()) {
                    add(parseSkillProposal(proposals.getJSONObject(index)))
                }
            }
        } finally {
            connection.disconnect()
        }
    }

    private fun getSkillsBlocking(baseUrl: String, deviceToken: String?): List<SkillProposal> {
        val connection = URL("$baseUrl/v1/skills?status=approved&limit=200").openConnection() as HttpURLConnection
        try {
            GatewayHttp.configure(connection, deviceToken)
            val skills = GatewayHttp.responseJson(connection).getJSONArray("skills")
            return buildList { for (index in 0 until skills.length()) add(parseSkillProposal(skills.getJSONObject(index))) }
        } finally {
            connection.disconnect()
        }
    }

    private fun getSkillUsagesBlocking(baseUrl: String, deviceToken: String?): List<SkillUsage> {
        val connection = URL("$baseUrl/v1/skills/usages?limit=30").openConnection() as HttpURLConnection
        try {
            GatewayHttp.configure(connection, deviceToken)
            val usages = GatewayHttp.responseJson(connection).getJSONArray("usages")
            return buildList {
                for (index in 0 until usages.length()) {
                    val usage = usages.getJSONObject(index)
                    add(SkillUsage(
                        id = usage.getString("id"),
                        skillId = usage.getString("skill_id"),
                        skillName = usage.getString("skill_name"),
                        skillVersion = usage.optInt("skill_version", 1),
                        outcome = usage.optString("outcome", "completed"),
                        feedback = usage.optString("feedback").takeIf { it.isNotBlank() && it != "null" },
                        usedAt = usage.optString("used_at"),
                    ))
                }
            }
        } finally {
            connection.disconnect()
        }
    }

    private fun setSkillEnabledBlocking(baseUrl: String, skillId: String, enabled: Boolean, deviceToken: String?): SkillProposal {
        val encodedId = URLEncoder.encode(skillId, Charsets.UTF_8.name())
        val request = JSONObject().put("status", if (enabled) "approved" else "disabled").toString().toByteArray(Charsets.UTF_8)
        val connection = URL("$baseUrl/v1/skills/$encodedId/status").openConnection() as HttpURLConnection
        try {
            GatewayHttp.configure(connection, deviceToken, request)
            return parseSkillProposal(GatewayHttp.responseJson(connection).getJSONObject("skill"))
        } finally {
            connection.disconnect()
        }
    }

    private fun reviewSkillUsageBlocking(baseUrl: String, usageId: String, correct: Boolean, deviceToken: String?): SkillUsage {
        val encodedId = URLEncoder.encode(usageId, Charsets.UTF_8.name())
        val request = JSONObject().put("feedback", if (correct) "correct" else "incorrect").toString().toByteArray(Charsets.UTF_8)
        val connection = URL("$baseUrl/v1/skills/usages/$encodedId/feedback").openConnection() as HttpURLConnection
        try {
            GatewayHttp.configure(connection, deviceToken, request)
            val usage = GatewayHttp.responseJson(connection).getJSONObject("usage")
            return SkillUsage(
                id = usage.getString("id"),
                skillId = usage.getString("skill_id"),
                skillName = usage.getString("skill_name"),
                skillVersion = usage.optInt("skill_version", 1),
                outcome = usage.optString("outcome", "completed"),
                feedback = usage.optString("feedback").takeIf { it.isNotBlank() && it != "null" },
                usedAt = usage.optString("used_at"),
            )
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
            GatewayHttp.configure(connection, deviceToken, request)
            return parseSkillProposal(GatewayHttp.responseJson(connection).getJSONObject("proposal"))
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

    private fun createTestOutreachBlocking(baseUrl: String, deviceToken: String?): VicOutreach {
        val request = JSONObject()
            .put("kind", "status_update")
            .put("priority", "check_in")
            .put("title", "VIC wants to talk")
            .put("body", "The proactive check-in system is connected. Tap Talk now to begin a conversation with me.")
            .put("reason", "Working-model delivery test requested from the VIC app")
            .put("dedupe_key", "android-working-model-${System.currentTimeMillis()}")
            .put("actions", org.json.JSONArray(listOf("talk_now", "show_progress", "later", "dismiss")))
            .toString().toByteArray(Charsets.UTF_8)
        val connection = URL("$baseUrl/v1/outreach").openConnection() as HttpURLConnection
        try {
            GatewayHttp.configure(connection, deviceToken, request)
            return parseOutreach(GatewayHttp.responseJson(connection).getJSONObject("outreach"))
        } finally {
            connection.disconnect()
        }
    }

    private fun getBridgeInboxBlocking(baseUrl: String, deviceToken: String?): List<BridgeNotification> {
        val connection = URL("$baseUrl/v1/bridge/inbox?limit=50").openConnection() as HttpURLConnection
        try {
            GatewayHttp.configure(connection, deviceToken)
            val records = GatewayHttp.responseJson(connection).getJSONArray("notifications")
            return buildList {
                for (index in 0 until records.length()) {
                    val item = records.getJSONObject(index)
                    add(BridgeNotification(item.getLong("id"), item.optJSONObject("payload") ?: JSONObject(), item.getString("created_at")))
                }
            }
        } finally {
            connection.disconnect()
        }
    }

    private fun getPendingOutreachBlocking(baseUrl: String, deviceToken: String?): List<VicOutreach> {
        val connection = URL("$baseUrl/v1/outreach?limit=50").openConnection() as HttpURLConnection
        try {
            GatewayHttp.configure(connection, deviceToken)
            val records = GatewayHttp.responseJson(connection).getJSONArray("outreach")
            return buildList {
                for (index in 0 until records.length()) {
                    val outreach = parseOutreach(records.getJSONObject(index))
                    if (outreach.status == "queued") add(outreach)
                }
            }
        } finally {
            connection.disconnect()
        }
    }

    private fun actOnOutreachBlocking(
        baseUrl: String,
        deviceToken: String?,
        outreachId: String,
        action: String,
        snoozeMinutes: Int?,
    ): VicOutreach {
        val encodedId = URLEncoder.encode(outreachId, Charsets.UTF_8.name())
        val payload = JSONObject().put("action", action).apply {
            if (snoozeMinutes != null) put("snooze_minutes", snoozeMinutes)
        }.toString().toByteArray(Charsets.UTF_8)
        val connection = URL("$baseUrl/v1/outreach/$encodedId/actions").openConnection() as HttpURLConnection
        try {
            GatewayHttp.configure(connection, deviceToken, payload)
            return parseOutreach(GatewayHttp.responseJson(connection).getJSONObject("outreach"))
        } finally {
            connection.disconnect()
        }
    }

    fun parseOutreach(payload: JSONObject): VicOutreach = VicOutreach(
        id = payload.getString("id"),
        kind = payload.optString("kind", "check_in"),
        priority = payload.optString("priority", "check_in"),
        title = payload.optString("title", "VIC wants to talk"),
        body = payload.optString("body"),
        reason = payload.optString("reason"),
        status = payload.optString("status", "queued"),
        taskId = payload.optString("task_id").takeIf { it.isNotBlank() },
    )

    private fun <T> retryConnection(action: () -> T): T {
        var lastError: IOException? = null
        repeat(3) { attempt ->
            try {
                return action()
            } catch (error: IOException) {
                lastError = error
                if (!GatewayTransportPolicy.canRetryWithoutDuplicatingRequest(error)) throw error
                if (attempt < 2) Thread.sleep(GatewayTransportPolicy.retryDelayMillis(attempt))
            }
        }
        throw lastError ?: IOException("Gateway connection failed")
    }
}
