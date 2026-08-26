package dev.voiceos.client

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class ConversationCommandsTest {
    @Test
    fun naturalStopPhrasesEndTheSession() {
        assertEquals(
            ConversationCommands.Action.STOP,
            ConversationCommands.action("VIC, end the conversation."),
        )
        assertEquals(
            ConversationCommands.Action.STOP,
            ConversationCommands.action("VIC, stop listening."),
        )
    }

    @Test
    fun pausePhrasesPauseWithoutEnding() {
        assertEquals(
            ConversationCommands.Action.PAUSE,
            ConversationCommands.action("VIC, pause."),
        )
    }

    @Test
    fun ordinaryDiscussionDoesNotAccidentallyStop() {
        assertNull(ConversationCommands.action("When should I stop listening to that podcast?"))
        assertNull(ConversationCommands.action("Tell me about conversational memory."))
        assertNull(ConversationCommands.action("That's all I have for the first topic."))
        assertNull(ConversationCommands.action("End the conversation section with a summary."))
    }
}
