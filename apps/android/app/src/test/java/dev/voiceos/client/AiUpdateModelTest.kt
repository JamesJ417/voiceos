package dev.voiceos.client

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class AiUpdateModelTest {
    @Test
    fun finiteFeedPrefersSourceVariety() {
        val selected = AiUpdateModel.select(
            listOf(
                update("openai-new", "OpenAI", 400),
                update("openai-old", "OpenAI", 300),
                update("deepmind", "Google DeepMind", 200),
                update("huggingface", "Hugging Face", 100),
            ),
            limit = 3,
        )

        assertEquals(listOf("openai-new", "deepmind", "huggingface"), selected.map { it.stableId })
    }

    @Test
    fun classifiesVideosLaunchesAndReports() {
        assertEquals(AiUpdateKind.VIDEO, AiUpdateModel.classify("Weekly update", video = true))
        assertEquals(AiUpdateKind.LAUNCH, AiUpdateModel.classify("Introducing a new model", video = false))
        assertEquals(AiUpdateKind.REPORT, AiUpdateModel.classify("New safety research report", video = false))
    }

    @Test
    fun officialVideosUsePrivacyEnhancedPlayer() {
        val url = AiUpdateModel.videoReaderUrl("abc123")

        assertTrue(url.startsWith("https://www.youtube-nocookie.com/embed/abc123"))
        assertTrue(url.contains("rel=0"))
    }

    @Test
    fun ageLabelsStaySimple() {
        val now = 1_000_000L

        assertEquals("TODAY", AiUpdateModel.ageLabel(now - 100, now))
        assertEquals("YESTERDAY", AiUpdateModel.ageLabel(now - 86_400, now))
        assertEquals("3 DAYS AGO", AiUpdateModel.ageLabel(now - (3 * 86_400), now))
    }

    private fun update(id: String, source: String, published: Long) = AiUpdate(
        stableId = id,
        source = source,
        title = id,
        summary = "Summary",
        readerUrl = "https://openai.com/$id",
        publishedEpochSeconds = published,
        kind = AiUpdateKind.NEWS,
    )
}
