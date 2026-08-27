package dev.voiceos.client

/** In-memory, explicit-readout-only worker report queue. */
class BackgroundConversationUpdateQueue {
    private val reports = LinkedHashMap<String, String>()

    val pendingCount: Int get() = reports.size

    fun enqueue(eventId: String, report: String) {
        if (eventId.isBlank() || report.isBlank()) return
        reports.putIfAbsent(eventId, report)
    }

    fun drain(): List<String> = reports.values.toList().also { reports.clear() }
}
