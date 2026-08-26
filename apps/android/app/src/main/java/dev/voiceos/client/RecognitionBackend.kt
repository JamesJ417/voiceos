package dev.voiceos.client

internal enum class RecognitionBackend {
    ON_DEVICE,
    PLATFORM;

    fun afterStall(): RecognitionBackend = PLATFORM

    companion object {
        fun fromPersisted(value: String?): RecognitionBackend =
            entries.firstOrNull { it.name.equals(value, ignoreCase = true) } ?: PLATFORM
    }
}
