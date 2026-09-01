package dev.voiceos.client

import java.util.Locale

data class ConversationArea(
    val id: String,
    val displayName: String,
    val position: Int,
)

data class AreaConversation(
    val id: String,
    val areaId: String,
    val title: String,
    val status: String,
    val messageCount: Long,
    val lastMessagePreview: String?,
    val updatedAt: String,
)

data class ConversationAreaState(
    val areas: List<ConversationArea>,
    val selectedAreaId: String,
    val activeConversation: AreaConversation?,
    val conversations: List<AreaConversation> = emptyList(),
    val syncCursor: Long = 0,
) {
    val selectedArea: ConversationArea
        get() = areas.firstOrNull { it.id == selectedAreaId }
            ?: ConversationAreaModel.generalTalk

    fun conversationsInSelectedArea(): List<AreaConversation> = conversations
        .filter { it.areaId == selectedAreaId }
        .sortedWith(compareByDescending<AreaConversation> { it.updatedAt }.thenBy { it.id })

    fun withServerSelection(areaId: String, conversation: AreaConversation?): ConversationAreaState {
        if (areas.none { it.id == areaId }) return this
        return copy(selectedAreaId = areaId, activeConversation = conversation)
    }

    fun withConversations(records: List<AreaConversation>): ConversationAreaState =
        copy(conversations = records.distinctBy { it.id })
}

sealed interface ConversationAreaVoiceCommand {
    data class Select(val areaId: String) : ConversationAreaVoiceCommand
    data class Create(val areaId: String) : ConversationAreaVoiceCommand
    data class RequestMove(val areaId: String) : ConversationAreaVoiceCommand
}

object ConversationAreaModel {
    val builtIns = listOf(
        ConversationArea("general-talk", "General Talk", 0),
        ConversationArea("brick-copper", "Brick & Copper", 1),
        ConversationArea("vine-branch-deli", "Vine and Branch Deli", 2),
        ConversationArea("sb-dom-online-ai", "S&B / Dom / Online AI", 3),
        ConversationArea("personal", "Personal", 4),
        ConversationArea("religious-biblical", "Religious / Biblical", 5),
    )
    val generalTalk: ConversationArea get() = builtIns.first()

    fun fromBootstrap(
        serverAreas: List<ConversationArea>,
        selectedAreaId: String?,
        activeConversation: AreaConversation?,
    ): ConversationAreaState {
        val canonical = serverAreas
            .filter { candidate -> builtIns.any { it.id == candidate.id } }
            .sortedBy { it.position }
            .takeIf { it.map(ConversationArea::id) == builtIns.map(ConversationArea::id) }
            ?: builtIns
        val selected = selectedAreaId?.takeIf { id -> canonical.any { it.id == id } }
            ?: generalTalk.id
        return ConversationAreaState(canonical, selected, activeConversation)
    }

    fun moveConfirmation(
        conversation: AreaConversation,
        destination: ConversationArea,
    ): String? {
        val source = builtIns.firstOrNull { it.id == conversation.areaId } ?: return null
        if (source.id == destination.id) return null
        return "Move “${conversation.title}” from ${source.displayName} to ${destination.displayName}? Its full message history will move with it."
    }

    fun parseVoiceCommand(text: String): ConversationAreaVoiceCommand? {
        val normalized = text.lowercase(Locale.US)
            .replace("&", "and")
            .replace(Regex("[^a-z0-9/ ]"), " ")
            .replace(Regex("\\s+"), " ")
            .trim()
        val prefix = when {
            normalized.startsWith("switch to ") -> "switch to " to "select"
            normalized.startsWith("open area ") -> "open area " to "select"
            normalized.startsWith("new conversation in ") -> "new conversation in " to "create"
            normalized.startsWith("start a conversation in ") -> "start a conversation in " to "create"
            normalized.startsWith("move this conversation to ") -> "move this conversation to " to "move"
            else -> return null
        }
        val spokenArea = normalized.removePrefix(prefix.first).trim()
        val area = builtIns.firstOrNull { normalizeAreaName(it.displayName) == spokenArea }
            ?: return null
        return when (prefix.second) {
            "select" -> ConversationAreaVoiceCommand.Select(area.id)
            "create" -> ConversationAreaVoiceCommand.Create(area.id)
            else -> ConversationAreaVoiceCommand.RequestMove(area.id)
        }
    }

    private fun normalizeAreaName(value: String): String = value.lowercase(Locale.US)
        .replace("&", "and")
        .replace(Regex("[^a-z0-9/ ]"), " ")
        .replace(Regex("\\s+"), " ")
        .trim()
}
