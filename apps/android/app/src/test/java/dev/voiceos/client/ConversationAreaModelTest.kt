package dev.voiceos.client

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class ConversationAreaModelTest {
    @Test
    fun canonicalAreasAndFallbackAreStable() {
        val state = ConversationAreaModel.fromBootstrap(emptyList(), "unknown", null)
        assertEquals(6, state.areas.size)
        assertEquals("general-talk", state.selectedAreaId)
        assertEquals("Religious / Biblical", state.areas.last().displayName)
    }

    @Test
    fun selectedAreaFilteringNeverMovesAConversation() {
        val general = AreaConversation("g", "general-talk", "General", "archived", 2, null, "2026-08-01")
        val personal = AreaConversation("p", "personal", "Journal", "active", 4, null, "2026-08-02")
        val state = ConversationAreaModel.fromBootstrap(
            ConversationAreaModel.builtIns,
            "personal",
            personal,
        ).withConversations(listOf(general, personal))

        assertEquals(listOf("p"), state.conversationsInSelectedArea().map { it.id })
        assertEquals("general-talk", general.areaId)
    }

    @Test
    fun moveRequiresDifferentKnownSourceAndShowsBothNames() {
        val conversation = AreaConversation("p", "personal", "Journal", "active", 1, null, "now")
        val destination = ConversationAreaModel.builtIns.first()
        val prompt = ConversationAreaModel.moveConfirmation(conversation, destination)

        assertTrue(prompt!!.contains("Personal"))
        assertTrue(prompt.contains("General Talk"))
        assertNull(
            ConversationAreaModel.moveConfirmation(
                conversation,
                ConversationAreaModel.builtIns.first { it.id == "personal" },
            ),
        )
    }

    @Test
    fun onlyExplicitAreaPhrasesBecomeVoiceCommands() {
        assertEquals(
            ConversationAreaVoiceCommand.Select("brick-copper"),
            ConversationAreaModel.parseVoiceCommand("Switch to Brick & Copper"),
        )
        assertEquals(
            ConversationAreaVoiceCommand.Create("religious-biblical"),
            ConversationAreaModel.parseVoiceCommand("New conversation in Religious / Biblical"),
        )
        assertEquals(
            ConversationAreaVoiceCommand.RequestMove("personal"),
            ConversationAreaModel.parseVoiceCommand("Move this conversation to Personal"),
        )
        assertNull(ConversationAreaModel.parseVoiceCommand("Let's discuss a personal question"))
    }
}
