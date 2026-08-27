package dev.voiceos.client

/** Builds the deterministic pre-processing acknowledgment spoken before gateway work. */
object ConversationAcknowledgment {
    data class Message(val text: String, val estimateMinutes: Int)

    fun forRequest(request: String): Message {
        val normalized = request.trim().replace(Regex("\\s+"), " ")
        val estimate = when {
            normalized.length <= 40 -> 1
            normalized.length <= 160 -> 3
            normalized.length <= 500 -> 5
            else -> 10
        }
        val plan = when {
            normalized.startsWith("how ", ignoreCase = true) || normalized.endsWith("?") -> "I’ll understand the question and give you a direct answer."
            normalized.contains("build", ignoreCase = true) || normalized.contains("create", ignoreCase = true) -> "I’ll inspect what exists, make the change, and verify it."
            normalized.contains("fix", ignoreCase = true) || normalized.contains("troubleshoot", ignoreCase = true) -> "I’ll inspect the failure, identify the cause, and test the fix."
            else -> "I’ll interpret your request, take the safe next step, and report the result."
        }
        return Message("Got it. $plan This should take about $estimate minute${if (estimate == 1) "" else "s"}.", estimate)
    }
}
