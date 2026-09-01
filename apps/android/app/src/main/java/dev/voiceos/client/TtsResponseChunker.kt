package dev.voiceos.client

/** Splits long replies below Android TTS limits without dropping or reordering text. */
internal object TtsResponseChunker {
    const val MAX_CHARS = 3_200

    fun split(text: String, maxChars: Int = MAX_CHARS): List<String> {
        val normalized = text.trim()
        if (normalized.isEmpty()) return emptyList()
        require(maxChars >= 32)
        val chunks = mutableListOf<String>()
        var remaining = normalized
        while (remaining.length > maxChars) {
            val window = remaining.take(maxChars + 1)
            val sentenceSplit = listOf(
                window.lastIndexOf(". "),
                window.lastIndexOf("? "),
                window.lastIndexOf("! "),
            ).maxOrNull()?.takeIf { it >= maxChars / 2 }
            val splitAt = sentenceSplit
                ?: window.lastIndexOf("\n").takeIf { it >= maxChars / 2 }
                ?: window.lastIndexOf(" ").takeIf { it >= maxChars / 2 }
                ?: maxChars
            val end = if (splitAt < window.length && window.getOrNull(splitAt) in listOf('.', '?', '!')) {
                splitAt + 1
            } else {
                splitAt
            }
            chunks += remaining.take(end).trim()
            remaining = remaining.drop(end).trimStart()
        }
        if (remaining.isNotEmpty()) chunks += remaining
        return chunks
    }
}
