package dev.voiceos.client

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class InterestCommandsTest {
    @Test
    fun extractsExplicitInterestCommands() {
        assertEquals("woodworking", InterestCommands.followTopic("VIC, follow my interest in woodworking"))
        assertEquals("AI agents", InterestCommands.followTopic("Follow this interest: AI agents"))
    }

    @Test
    fun doesNotMistakeOrdinaryFollowUpsForInterests() {
        assertNull(InterestCommands.followTopic("Follow up with Lee tomorrow"))
        assertNull(InterestCommands.followTopic("Follow this interest"))
    }
}
