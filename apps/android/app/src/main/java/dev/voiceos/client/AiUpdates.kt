package dev.voiceos.client

import android.content.Context
import org.json.JSONArray
import org.json.JSONObject
import org.xmlpull.v1.XmlPullParser
import org.xmlpull.v1.XmlPullParserFactory
import java.net.HttpURLConnection
import java.net.URL
import java.time.Instant
import java.time.ZonedDateTime
import java.time.format.DateTimeFormatter
import java.util.concurrent.Callable
import java.util.concurrent.Executors

enum class AiUpdateKind { VIDEO, LAUNCH, REPORT, NEWS }

data class AiUpdate(
    val stableId: String,
    val source: String,
    val title: String,
    val summary: String,
    val readerUrl: String,
    val publishedEpochSeconds: Long,
    val kind: AiUpdateKind,
)

data class AiUpdateSource(
    val name: String,
    val feedUrl: String,
    val video: Boolean = false,
)

object AiUpdateModel {
    fun select(updates: List<AiUpdate>, limit: Int = 4): List<AiUpdate> {
        val safeLimit = limit.coerceIn(1, 8)
        val ranked = updates
            .filter { it.readerUrl.startsWith("https://") }
            .distinctBy(AiUpdate::stableId)
            .sortedByDescending(AiUpdate::publishedEpochSeconds)
        val selected = mutableListOf<AiUpdate>()
        ranked.forEach { update ->
            if (selected.size < safeLimit && selected.none { it.source == update.source }) selected += update
        }
        ranked.forEach { update ->
            if (selected.size < safeLimit && selected.none { it.stableId == update.stableId }) selected += update
        }
        return selected
    }

    fun classify(title: String, video: Boolean): AiUpdateKind {
        if (video) return AiUpdateKind.VIDEO
        val normalized = title.lowercase()
        return when {
            listOf("launch", "introducing", "release", "available", "new model").any(normalized::contains) -> AiUpdateKind.LAUNCH
            listOf("research", "report", "paper", "study", "benchmark", "evaluation").any(normalized::contains) -> AiUpdateKind.REPORT
            else -> AiUpdateKind.NEWS
        }
    }

    fun videoReaderUrl(videoId: String): String =
        "https://www.youtube-nocookie.com/embed/$videoId?rel=0&modestbranding=1"

    fun ageLabel(publishedEpochSeconds: Long, nowEpochSeconds: Long = Instant.now().epochSecond): String {
        if (publishedEpochSeconds <= 0) return "RECENT"
        val days = ((nowEpochSeconds - publishedEpochSeconds).coerceAtLeast(0) / 86_400).toInt()
        return when (days) {
            0 -> "TODAY"
            1 -> "YESTERDAY"
            in 2..13 -> "$days DAYS AGO"
            else -> "RECENT"
        }
    }
}

object AiUpdateRepository {
    private val sources = listOf(
        AiUpdateSource("OpenAI", "https://openai.com/news/rss.xml"),
        AiUpdateSource("Google DeepMind", "https://deepmind.google/blog/rss.xml"),
        AiUpdateSource("Hugging Face", "https://huggingface.co/blog/feed.xml"),
        AiUpdateSource(
            "OpenAI Video",
            "https://www.youtube.com/feeds/videos.xml?channel_id=UCXZCJLdBC09xxGZ6gcdrc6A",
            video = true,
        ),
    )

    fun refresh(onComplete: (Result<List<AiUpdate>>) -> Unit) {
        Thread({
            val executor = Executors.newFixedThreadPool(sources.size)
            try {
                val futures = sources.map { source ->
                    executor.submit(Callable { runCatching { fetch(source) }.getOrDefault(emptyList()) })
                }
                val updates = futures.flatMap { future -> runCatching { future.get() }.getOrDefault(emptyList()) }
                if (updates.isEmpty()) {
                    onComplete(Result.failure(IllegalStateException("No official AI updates available")))
                } else {
                    onComplete(Result.success(updates.sortedByDescending(AiUpdate::publishedEpochSeconds).take(20)))
                }
            } finally {
                executor.shutdownNow()
            }
        }, "ov-ai-updates").start()
    }

    private fun fetch(source: AiUpdateSource): List<AiUpdate> {
        val connection = URL(source.feedUrl).openConnection() as HttpURLConnection
        return try {
            connection.connectTimeout = 8_000
            connection.readTimeout = 10_000
            connection.instanceFollowRedirects = true
            connection.setRequestProperty("Accept", "application/rss+xml, application/atom+xml, application/xml, text/xml")
            connection.setRequestProperty("User-Agent", "OmarchyVoice/0.9 Android")
            connection.inputStream.buffered().use { input -> parse(input.reader(), source) }
        } finally {
            connection.disconnect()
        }
    }

    internal fun parse(reader: java.io.Reader, source: AiUpdateSource): List<AiUpdate> {
        val parser = XmlPullParserFactory.newInstance().newPullParser().apply { setInput(reader) }
        val updates = mutableListOf<AiUpdate>()
        var insideEntry = false
        var title = ""
        var link = ""
        var summary = ""
        var published = ""
        var videoId = ""
        var event = parser.eventType
        while (event != XmlPullParser.END_DOCUMENT) {
            val name = parser.name.orEmpty()
            when (event) {
                XmlPullParser.START_TAG -> when (name) {
                    "item", "entry" -> {
                        insideEntry = true
                        title = ""
                        link = ""
                        summary = ""
                        published = ""
                        videoId = ""
                    }
                    "title" -> if (insideEntry) title = parser.nextText().trim()
                    "link" -> if (insideEntry) {
                        link = parser.getAttributeValue(null, "href")?.trim().orEmpty()
                            .ifBlank { parser.nextText().trim() }
                    }
                    "description", "summary" -> if (insideEntry && summary.isBlank()) summary = parser.nextText().trim()
                    "pubDate", "published", "updated" -> if (insideEntry && published.isBlank()) published = parser.nextText().trim()
                    "videoId" -> if (insideEntry) videoId = parser.nextText().trim()
                }
                XmlPullParser.END_TAG -> if (name == "item" || name == "entry") {
                    insideEntry = false
                    val resolvedUrl = when {
                        source.video && videoId.isNotBlank() -> AiUpdateModel.videoReaderUrl(videoId)
                        else -> link
                    }
                    if (title.isNotBlank() && resolvedUrl.startsWith("https://")) {
                        updates += AiUpdate(
                            stableId = "${source.name}:${videoId.ifBlank { link }}",
                            source = source.name,
                            title = clean(title, 150),
                            summary = clean(summary, 240).ifBlank {
                                if (source.video) "Official OpenAI release video." else "Official update from ${source.name}."
                            },
                            readerUrl = resolvedUrl,
                            publishedEpochSeconds = parseDate(published),
                            kind = AiUpdateModel.classify(title, source.video),
                        )
                    }
                }
            }
            event = parser.next()
        }
        return updates.take(12)
    }

    private fun clean(value: String, maxLength: Int): String = value
        .replace(Regex("<[^>]+>"), " ")
        .replace("&nbsp;", " ")
        .replace("&amp;", "&")
        .replace(Regex("\\s+"), " ")
        .trim()
        .let { if (it.length <= maxLength) it else it.take(maxLength - 1).trimEnd() + "…" }

    private fun parseDate(value: String): Long {
        if (value.isBlank()) return 0
        return runCatching { Instant.parse(value).epochSecond }.getOrElse {
            runCatching { ZonedDateTime.parse(value, DateTimeFormatter.RFC_1123_DATE_TIME).toEpochSecond() }.getOrDefault(0)
        }
    }
}

object AiUpdateStore {
    private const val PREFERENCES = "ov_ai_updates"
    private const val ITEMS = "items"
    private const val UPDATED_AT = "updated_at"
    private const val MAX_CACHE_AGE_MS = 30L * 60L * 1_000L

    fun save(context: Context, updates: List<AiUpdate>) {
        val payload = JSONArray()
        updates.take(20).forEach { update ->
            payload.put(JSONObject().apply {
                put("id", update.stableId)
                put("source", update.source)
                put("title", update.title)
                put("summary", update.summary)
                put("url", update.readerUrl)
                put("published", update.publishedEpochSeconds)
                put("kind", update.kind.name)
            })
        }
        context.getSharedPreferences(PREFERENCES, Context.MODE_PRIVATE).edit()
            .putString(ITEMS, payload.toString())
            .putLong(UPDATED_AT, System.currentTimeMillis())
            .apply()
    }

    fun load(context: Context): List<AiUpdate> {
        val raw = context.getSharedPreferences(PREFERENCES, Context.MODE_PRIVATE).getString(ITEMS, null) ?: return emptyList()
        return runCatching {
            val payload = JSONArray(raw)
            buildList {
                for (index in 0 until payload.length()) {
                    val item = payload.getJSONObject(index)
                    add(
                        AiUpdate(
                            stableId = item.getString("id"),
                            source = item.getString("source"),
                            title = item.getString("title"),
                            summary = item.getString("summary"),
                            readerUrl = item.getString("url"),
                            publishedEpochSeconds = item.optLong("published"),
                            kind = runCatching { AiUpdateKind.valueOf(item.getString("kind")) }.getOrDefault(AiUpdateKind.NEWS),
                        ),
                    )
                }
            }
        }.getOrDefault(emptyList())
    }

    fun isFresh(context: Context): Boolean {
        val updatedAt = context.getSharedPreferences(PREFERENCES, Context.MODE_PRIVATE).getLong(UPDATED_AT, 0)
        return updatedAt > 0 && System.currentTimeMillis() - updatedAt < MAX_CACHE_AGE_MS
    }
}
