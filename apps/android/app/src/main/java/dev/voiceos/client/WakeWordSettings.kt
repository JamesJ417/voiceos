package dev.voiceos.client

import android.content.Context

object WakeWordSettings {
    private const val PREFERENCES = "vic_wake_word"
    private const val ENABLED = "enabled"

    const val KEYWORD = "Hey VIC"
    const val COOLDOWN_MILLIS = 8_000L
    const val RESTART_DELAY_MILLIS = 1_500L
    const val MIN_SIGNAL_PEAK = 0.018f
    const val KEYWORD_SCORE = 1.8f
    const val KEYWORD_THRESHOLD = 0.35f

    fun isEnabled(context: Context): Boolean = context
        .getSharedPreferences(PREFERENCES, Context.MODE_PRIVATE)
        .getBoolean(ENABLED, false)

    fun setEnabled(context: Context, enabled: Boolean) {
        context.getSharedPreferences(PREFERENCES, Context.MODE_PRIVATE)
            .edit().putBoolean(ENABLED, enabled).apply()
    }
}

object WakeWordPolicy {
    fun shouldActivate(
        keyword: String,
        signalPeak: Float,
        nowMillis: Long,
        lastActivationMillis: Long,
        conversationActive: Boolean,
    ): Boolean = keyword.isNotBlank() &&
        !conversationActive &&
        signalPeak >= WakeWordSettings.MIN_SIGNAL_PEAK &&
        nowMillis - lastActivationMillis >= WakeWordSettings.COOLDOWN_MILLIS
}
