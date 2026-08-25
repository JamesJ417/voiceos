package dev.voiceos.client

import android.content.Context
import org.json.JSONArray
import org.json.JSONObject
import java.util.Locale
import java.util.UUID

data class VicInterest(
    val id: String,
    val topic: String,
    val createdAtMillis: Long,
)

object InterestStore {
    private const val PREFERENCES = "vic_interests"
    private const val INTERESTS = "interests"

    fun list(context: Context): List<VicInterest> {
        val raw = context.getSharedPreferences(PREFERENCES, Context.MODE_PRIVATE)
            .getString(INTERESTS, null)
            ?: return emptyList()
        return runCatching {
            val array = JSONArray(raw)
            buildList {
                for (index in 0 until array.length()) {
                    val item = array.getJSONObject(index)
                    add(
                        VicInterest(
                            id = item.getString("id"),
                            topic = item.getString("topic"),
                            createdAtMillis = item.getLong("created_at_millis"),
                        ),
                    )
                }
            }
        }.getOrDefault(emptyList())
    }

    fun follow(context: Context, topic: String): VicInterest {
        val normalized = topic.trim().replace(Regex("\\s+"), " ").take(80)
        require(normalized.isNotBlank()) { "Interest topic is required" }
        val current = list(context)
        current.firstOrNull { it.topic.equals(normalized, ignoreCase = true) }?.let { return it }
        val interest = VicInterest(UUID.randomUUID().toString(), normalized, System.currentTimeMillis())
        save(context, listOf(interest) + current)
        return interest
    }

    fun unfollow(context: Context, interestId: String) {
        save(context, list(context).filterNot { it.id == interestId })
    }

    private fun save(context: Context, interests: List<VicInterest>) {
        val encoded = JSONArray().apply {
            interests.take(30).forEach { interest ->
                put(
                    JSONObject()
                        .put("id", interest.id)
                        .put("topic", interest.topic)
                        .put("created_at_millis", interest.createdAtMillis),
                )
            }
        }
        context.getSharedPreferences(PREFERENCES, Context.MODE_PRIVATE)
            .edit().putString(INTERESTS, encoded.toString()).apply()
    }
}

object InterestCommands {
    fun followTopic(text: String): String? {
        val normalized = text.trim()
            .replace(Regex("[,:;]+"), " ")
            .replace(Regex("\\s+"), " ")
        val lowercase = normalized.lowercase(Locale.US)
        val prefixes = listOf(
            "vic follow my interest in ",
            "follow my interest in ",
            "vic follow the interest ",
            "follow the interest ",
            "vic follow this interest ",
            "follow this interest ",
            "vic follow interest ",
            "follow interest ",
        )
        val prefix = prefixes.firstOrNull(lowercase::startsWith) ?: return null
        return normalized.substring(prefix.length).trim(' ', '.', ',', ':', ';')
            .takeIf { it.length in 2..80 }
    }
}
