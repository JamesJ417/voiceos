package dev.voiceos.client

import android.content.Context
import android.graphics.Typeface
import android.view.ViewGroup
import android.widget.Button
import android.widget.LinearLayout
import android.widget.TextView

data class SleepMemoryPanelViews(
    val root: LinearLayout,
    val status: TextView,
    val report: TextView,
)

data class DoctrinePanelViews(
    val root: LinearLayout,
    val status: TextView,
    val candidates: LinearLayout,
)

fun buildSleepMemoryPanel(
    context: Context,
    onDryRun: () -> Unit,
    onCommit: () -> Unit,
    onRollback: () -> Unit,
): SleepMemoryPanelViews {
    val status = operationText(context, "Loading sleep-memory status...")
    val report = operationText(context, "No morning report yet.")
    val root = context.carbonPanelLayout(17).apply {
        addView(context.carbonKicker("Reconstructive memory"), fullWidthWrapLayout())
        addView(context.carbonHeading("VIC sleep cycle", 22f).apply {
            setPadding(0, context.pixels(5), 0, 0)
        }, fullWidthWrapLayout())
        addView(status, fullWidthWrapLayout().topMargin(context, 12))
        addView(report, fullWidthWrapLayout().topMargin(context, 8))
        addView(systemButton(context, "DRY RUN", onDryRun), fullWidthWrapLayout().topMargin(context, 12))
        addView(systemButton(context, "RUN & COMMIT", onCommit), fullWidthWrapLayout().topMargin(context, 8))
        addView(systemButton(context, "ROLL BACK LAST CYCLE", onRollback), fullWidthWrapLayout().topMargin(context, 8))
    }
    return SleepMemoryPanelViews(root, status, report)
}

fun buildDoctrinePanel(context: Context): DoctrinePanelViews {
    val status = operationText(context, "Loading doctrine status...")
    val candidates = LinearLayout(context).apply { orientation = LinearLayout.VERTICAL }
    val root = context.carbonPanelLayout(17).apply {
        addView(context.carbonKicker("Protected reasoning architecture"), fullWidthWrapLayout())
        addView(context.carbonHeading("VIC doctrine", 22f).apply {
            setPadding(0, context.pixels(5), 0, 0)
        }, fullWidthWrapLayout())
        addView(status, fullWidthWrapLayout().topMargin(context, 12))
        addView(candidates, fullWidthWrapLayout().topMargin(context, 10))
        addView(TextView(context).apply {
            text = "Doctrine sources stay private. Model output creates protected review candidates; approval never activates them automatically."
            textSize = 13f
            setTextColor(CarbonPalette.muted)
            setPadding(0, context.pixels(10), 0, 0)
        }, fullWidthWrapLayout())
    }
    return DoctrinePanelViews(root, status, candidates)
}

private fun operationText(context: Context, value: String) = TextView(context).apply {
    text = value
    textSize = 13f
    setTextColor(CarbonPalette.white)
    setPadding(context.pixels(12), context.pixels(12), context.pixels(12), context.pixels(12))
    background = carbonControl(context, CarbonPalette.line)
}

private fun systemButton(context: Context, label: String, action: () -> Unit) = Button(context).apply {
    text = label
    textSize = 11f
    typeface = Typeface.DEFAULT_BOLD
    minHeight = context.pixels(50)
    setTextColor(CarbonPalette.white)
    background = carbonControl(context, CarbonPalette.line)
    stateListAnimator = null
    setOnClickListener { action() }
}

private fun Context.pixels(value: Int) = (value * resources.displayMetrics.density).toInt()

private fun LinearLayout.LayoutParams.topMargin(context: Context, value: Int) = apply {
    topMargin = context.pixels(value)
}
