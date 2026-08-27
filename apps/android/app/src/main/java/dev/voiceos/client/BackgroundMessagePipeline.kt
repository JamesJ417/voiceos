package dev.voiceos.client

/** Pure decision logic shared by the event stream, inbox, and notification surfaces. */
object BackgroundMessagePipeline {
    const val EVENT_TYPE = "background.message.created"

    fun stableId(event: ClientEvent): String = stableId(event.id)
    fun stableId(eventId: Long): String = "background:$eventId"

    fun isCompletedWorkerResult(event: ClientEvent): Boolean {
        return isCompletedWorkerResult(event.type, event.payload.optString("status"), event.payload.optString("provider"), event.payload.optString("response_text"))
    }

    fun isCompletedWorkerResult(type: String, statusValue: String, provider: String = "", responseText: String = ""): Boolean {
        if (type != "conversation.turn" && type != "agent.worker.updated") return false
        val status = statusValue.lowercase()
        return status == "completed" || (type == "conversation.turn" &&
            provider == "hermes-subagent" && responseText.isNotBlank())
    }

    fun report(event: ClientEvent): String = event.payload.optString("response_text").trim()

    fun shouldNotify(activeConversation: Boolean, alreadyDelivered: Boolean): Boolean =
        !alreadyDelivered

    fun shouldReadAloud(activeConversation: Boolean): Boolean = false
}
