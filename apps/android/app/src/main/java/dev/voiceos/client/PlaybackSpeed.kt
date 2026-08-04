package dev.voiceos.client

import java.util.Locale
import kotlin.math.abs

object PlaybackSpeed {
    const val DEFAULT = 1.25f
    const val MIN = 1f
    const val MAX = 2f

    private val rates = floatArrayOf(1f, 1.25f, 1.5f, 1.75f, 2f)

    fun clamp(rate: Float): Float = rate.coerceIn(MIN, MAX)

    fun next(current: Float): Float {
        val index = nearestIndex(current)
        return rates[(index + 1) % rates.size]
    }

    fun resolveCommand(text: String, current: Float): Float? {
        val normalized = text.lowercase(Locale.US)
            .replace(Regex("[^a-z0-9. ]"), " ")
            .replace(Regex("\\s+"), " ")
            .trim()
        return when {
            listOf("double speed", "two times", "2x", "2 x", "speed to two", "speed to 2")
                .any(normalized::contains) -> 2f
            listOf("one point seven five", "1.75", "speed to one seventy five")
                .any(normalized::contains) -> 1.75f
            listOf("one point five", "1.5", "speed to one and a half")
                .any(normalized::contains) -> 1.5f
            listOf("one point two five", "1.25").any(normalized::contains) -> 1.25f
            listOf("normal speed", "regular speed", "reset speech speed", "speak normally")
                .any(normalized::contains) -> 1f
            listOf("speak faster", "talk faster", "read faster", "speed up", "increase speech speed")
                .any(normalized::contains) -> adjacent(current, 1)
            listOf("speak slower", "talk slower", "read slower", "slow down", "decrease speech speed")
                .any(normalized::contains) -> adjacent(current, -1)
            else -> null
        }
    }

    fun label(rate: Float): String = when (clamp(rate)) {
        1f -> "1×"
        1.25f -> "1.25×"
        1.5f -> "1.5×"
        1.75f -> "1.75×"
        else -> "2×"
    }

    fun buttonLabel(rate: Float): String = "SPEED ${label(rate)}"

    private fun adjacent(current: Float, direction: Int): Float {
        val index = nearestIndex(current)
        return rates[(index + direction).coerceIn(rates.indices)]
    }

    private fun nearestIndex(rate: Float): Int = rates.indices.minByOrNull {
        abs(rates[it] - rate)
    } ?: 0
}
