package dev.voiceos.client

object ArticleListeningModel {
    fun prepare(title: String, source: String, summary: String, pageText: String): String {
        val cleanTitle = cleanInline(title)
        val cleanSource = cleanInline(source)
        val cleanSummary = cleanInline(summary)
        val cleanPage = pageText.lineSequence()
            .map(::cleanInline)
            .filter { it.length >= 2 }
            .filterNot(::isPageNoise)
            .distinctBy(String::lowercase)
            .joinToString("\n")
            .take(MAX_ARTICLE_CHARS)
            .trim()
        val pageWords = cleanPage.split(Regex("\\s+")).count(String::isNotBlank)
        val body = if (pageWords >= MIN_FULL_ARTICLE_WORDS) cleanPage else cleanSummary
        return listOf(
            cleanSource.takeIf(String::isNotBlank)?.let { "From $it." }.orEmpty(),
            cleanTitle,
            body,
        ).filter(String::isNotBlank)
            .joinToString("\n")
            .trim()
    }

    fun chunks(text: String, maxChars: Int = 3_500): List<String> {
        val limit = maxChars.coerceIn(200, 3_800)
        val pieces = text.trim().split(Regex("(?<=[.!?])\\s+|\\n+"))
            .map(String::trim)
            .filter(String::isNotBlank)
            .flatMap { splitOversized(it, limit) }
        val chunks = mutableListOf<String>()
        val current = StringBuilder()
        pieces.forEach { piece ->
            if (current.isNotEmpty() && current.length + piece.length + 1 > limit) {
                chunks += current.toString()
                current.clear()
            }
            if (current.isNotEmpty()) current.append(' ')
            current.append(piece)
        }
        if (current.isNotEmpty()) chunks += current.toString()
        return chunks
    }

    private fun splitOversized(value: String, limit: Int): List<String> {
        if (value.length <= limit) return listOf(value)
        val output = mutableListOf<String>()
        val current = StringBuilder()
        value.split(Regex("\\s+")).forEach { word ->
            if (current.isNotEmpty() && current.length + word.length + 1 > limit) {
                output += current.toString()
                current.clear()
            }
            if (current.isNotEmpty()) current.append(' ')
            current.append(word.take(limit))
        }
        if (current.isNotEmpty()) output += current.toString()
        return output
    }

    private fun cleanInline(value: String): String = value
        .replace(Regex("https?://\\S+"), "")
        .replace(Regex("\\s+"), " ")
        .trim()

    private fun isPageNoise(value: String): Boolean {
        val normalized = value.lowercase()
        return normalized in PAGE_NOISE || PAGE_NOISE_PREFIXES.any(normalized::startsWith)
    }

    private const val MIN_FULL_ARTICLE_WORDS = 60
    private const val MAX_ARTICLE_CHARS = 60_000
    private val PAGE_NOISE = setOf(
        "skip to content", "menu", "search", "close", "sign in", "log in", "subscribe",
        "accept", "reject", "privacy", "terms", "share", "copy link", "back to top",
    )
    private val PAGE_NOISE_PREFIXES = listOf(
        "cookie settings", "manage cookies", "all rights reserved", "follow us on",
        "sign up for", "subscribe to", "related articles", "recommended for you",
    )
}
