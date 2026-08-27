package dev.voiceos.client

/** In-memory, explicit-readout-only worker report queue. */
class BackgroundConversationUpdateQueue {
    private val reports = LinkedHashMap<String, String>()

    val pendingCount: Int get() = reports.size

    fun enqueue(eventId: String, report: String): Boolean {
        if (eventId.isBlank() || report.isBlank() || reports.containsKey(eventId)) return false
        reports.putIfAbsent(eventId, report)
        return true
    }

    fun drain(): List<String> = reports.values.toList().also { reports.clear() }
}
