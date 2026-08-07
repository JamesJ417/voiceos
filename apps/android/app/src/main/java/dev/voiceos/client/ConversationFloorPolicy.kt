package dev.voiceos.client

internal object ConversationFloorPolicy {
    fun shouldYield(
        sessionActive: Boolean,
        minimumRevision: Long,
        incoming: ConversationFloor,
        thisDeviceId: String?,
    ): Boolean = sessionActive &&
        incoming.active &&
        incoming.revision > minimumRevision &&
        !incoming.holderDeviceId.isNullOrBlank() &&
        incoming.holderDeviceId != thisDeviceId
}
