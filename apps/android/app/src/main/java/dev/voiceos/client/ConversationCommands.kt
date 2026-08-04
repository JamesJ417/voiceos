package dev.voiceos.client

import java.util.Locale

object ConversationCommands {
    enum class Action { STOP, PAUSE }

    fun action(text: String): Action? = when (normalize(text)) {
        in STOP_COMMANDS -> Action.STOP
        in PAUSE_COMMANDS -> Action.PAUSE
        else -> null
    }

    fun normalize(text: String): String = text.lowercase(Locale.US)
        .replace(Regex("[^a-z0-9 ]"), " ")
        .replace(Regex("\\s+"), " ")
        .trim()

    private val STOP_COMMANDS = setOf(
        "stop listening",
        "vic stop listening",
        "end conversation",
        "end the conversation",
        "vic end the conversation",
        "that s all",
        "goodbye vic",
    )
    private val PAUSE_COMMANDS = setOf(
        "pause",
        "pause conversation",
        "pause the conversation",
        "vic pause",
    )
}
