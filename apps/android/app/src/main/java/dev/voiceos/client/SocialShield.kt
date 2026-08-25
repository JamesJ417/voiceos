package dev.voiceos.client

import android.accessibilityservice.AccessibilityService
import android.content.Context
import android.content.Intent
import android.view.accessibility.AccessibilityEvent

object SocialShieldStore {
    private const val PREFERENCES = "vic_social_shield"
    private const val PACKAGES = "packages"
    private const val BYPASS_PACKAGE = "bypass_package"
    private const val BYPASS_UNTIL = "bypass_until"

    fun packages(context: Context): Set<String> = context
        .getSharedPreferences(PREFERENCES, Context.MODE_PRIVATE)
        .getStringSet(PACKAGES, emptySet())
        ?.toSet()
        .orEmpty()

    fun setPackages(context: Context, packages: Set<String>) {
        context.getSharedPreferences(PREFERENCES, Context.MODE_PRIVATE)
            .edit().putStringSet(PACKAGES, packages).apply()
    }

    fun allowTemporarily(context: Context, packageName: String, minutes: Int = 10) {
        context.getSharedPreferences(PREFERENCES, Context.MODE_PRIVATE).edit()
            .putString(BYPASS_PACKAGE, packageName)
            .putLong(BYPASS_UNTIL, System.currentTimeMillis() + minutes * 60_000L)
            .apply()
    }

    fun isBypassed(context: Context, packageName: String): Boolean {
        val preferences = context.getSharedPreferences(PREFERENCES, Context.MODE_PRIVATE)
        return preferences.getString(BYPASS_PACKAGE, null) == packageName &&
            preferences.getLong(BYPASS_UNTIL, 0L) > System.currentTimeMillis()
    }
}

class SocialShieldAccessibilityService : AccessibilityService() {
    override fun onAccessibilityEvent(event: AccessibilityEvent?) {
        if (event?.eventType != AccessibilityEvent.TYPE_WINDOW_STATE_CHANGED) return
        val openedPackage = event.packageName?.toString()?.takeIf(String::isNotBlank) ?: return
        if (openedPackage == packageName) return
        if (openedPackage !in SocialShieldStore.packages(this)) return
        if (SocialShieldStore.isBypassed(this, openedPackage)) return
        if (FocusWidgetModel.select(TaskWidgetStore.load(this)).primary == null) return
        startActivity(
            Intent(this, MainActivity::class.java).apply {
                action = MainActivity.ACTION_SOCIAL_SHIELD
                flags = Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_CLEAR_TOP or
                    Intent.FLAG_ACTIVITY_SINGLE_TOP
                putExtra(MainActivity.EXTRA_BLOCKED_PACKAGE, openedPackage)
            },
        )
    }

    override fun onInterrupt() = Unit
}
