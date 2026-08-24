package dev.voiceos.client

import android.content.Context

interface OutreachEventTransport {
    fun start(after: Long, onOutreach: (Long, VicOutreach) -> Unit, onClosed: (Throwable?) -> Unit)
    fun stop()
}

class SseOutreachTransport(private val context: Context) : OutreachEventTransport {
    private var subscription: EventSubscription? = null

    override fun start(after: Long, onOutreach: (Long, VicOutreach) -> Unit, onClosed: (Throwable?) -> Unit) {
        val token = DeviceCredentials.token(context)
        if (token.isNullOrBlank()) {
            onClosed(IllegalStateException("VoiceOS enrollment is required"))
            return
        }
        subscription = GatewayClient.streamEvents(
            GatewaySettings.baseUrl(context), token, after,
            onEvent = { event ->
                if (event.type == "vic.outreach.created") {
                    onOutreach(event.id, GatewayClient.parseOutreach(event.payload))
                }
            },
            onClosed = onClosed,
        )
    }

    override fun stop() {
        subscription?.close()
        subscription = null
    }
}
