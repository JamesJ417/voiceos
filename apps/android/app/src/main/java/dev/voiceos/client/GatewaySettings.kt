package dev.voiceos.client

import android.content.Context
import android.content.Intent
import android.net.Uri

data class GatewayEnrollment(val baseUrl: String, val code: String?)

object GatewaySettings {
    private const val PREFERENCES = "voiceos_gateway"
    private const val BASE_URL = "base_url"

    fun baseUrl(context: Context): String = context
        .getSharedPreferences(PREFERENCES, Context.MODE_PRIVATE)
        .getString(BASE_URL, null)
        ?.takeIf { it.isNotBlank() }
        ?: BuildConfig.GATEWAY_BASE_URL

    fun enrollFromIntent(context: Context, intent: Intent?): GatewayEnrollment? {
        val data = intent?.data ?: return null
        if (data.scheme != "voiceos" || data.host != "enroll") return null
        val candidate = data.getQueryParameter("gateway")?.trim()?.trimEnd('/') ?: return null
        val parsed = Uri.parse(candidate)
        if (parsed.scheme != "https" || parsed.host.isNullOrBlank() || parsed.userInfo != null) {
            return null
        }
        context.getSharedPreferences(PREFERENCES, Context.MODE_PRIVATE)
            .edit()
            .putString(BASE_URL, candidate)
            .commit()
        val code = data.getQueryParameter("code")?.trim()?.takeIf { it.isNotEmpty() }
        return GatewayEnrollment(candidate, code)
    }

    fun displayName(context: Context): String {
        val uri = Uri.parse(baseUrl(context))
        return uri.host ?: baseUrl(context)
    }
}
