package dev.voiceos.client

/** Uses elapsed time, rather than recognizer callback counts, to decide when a session is idle. */
internal class ConversationSilencePolicy(
    private val inactivityTimeoutMillis: Long,
    private val clock: () -> Long,
) {
    private var lastActivityMillis = clock()

    init {
        require(inactivityTimeoutMillis > 0L) { "inactivityTimeoutMillis must be positive" }
    }

    fun markActivity() {
        lastActivityMillis = clock()
    }

    fun idleDurationMillis(): Long = (clock() - lastActivityMillis).coerceAtLeast(0L)

    fun shouldEndConversation(): Boolean = idleDurationMillis() >= inactivityTimeoutMillis
}
