package dev.voiceos.client

import android.content.Context
import android.graphics.Canvas
import android.graphics.Color
import android.graphics.Paint
import android.graphics.Path
import android.graphics.RectF
import android.graphics.Typeface
import android.os.SystemClock
import android.util.TypedValue
import android.view.View
import java.util.Locale
import kotlin.math.PI
import kotlin.math.sin

class AgentSignalView(context: Context) : View(context) {
    private val density = resources.displayMetrics.density
    private val frame = RectF()
    private val clipPath = Path()
    private val signalPath = Path()
    private val fillPaint = Paint(Paint.ANTI_ALIAS_FLAG).apply { color = CarbonPalette.black }
    private val borderPaint = Paint(Paint.ANTI_ALIAS_FLAG).apply {
        color = CarbonPalette.cyan
        style = Paint.Style.STROKE
        strokeWidth = density
    }
    private val gridPaint = Paint(Paint.ANTI_ALIAS_FLAG).apply {
        color = Color.argb(54, 69, 207, 228)
        style = Paint.Style.STROKE
        strokeWidth = density * 0.7f
    }
    private val scanPaint = Paint(Paint.ANTI_ALIAS_FLAG).apply {
        color = Color.argb(88, 53, 224, 193)
        strokeWidth = density * 1.2f
    }
    private val signalGlowPaint = Paint(Paint.ANTI_ALIAS_FLAG).apply {
        color = Color.argb(42, 53, 224, 193)
        style = Paint.Style.STROKE
        strokeWidth = density * 7f
    }
    private val signalPaint = Paint(Paint.ANTI_ALIAS_FLAG).apply {
        color = CarbonPalette.teal
        style = Paint.Style.STROKE
        strokeWidth = density * 1.8f
    }
    private val titlePaint = Paint(Paint.ANTI_ALIAS_FLAG).apply {
        color = CarbonPalette.white
        textSize = sp(12f)
        typeface = Typeface.create(Typeface.MONOSPACE, Typeface.BOLD)
        letterSpacing = 0.08f
    }
    private val labelPaint = Paint(Paint.ANTI_ALIAS_FLAG).apply {
        color = CarbonPalette.teal
        textSize = sp(10f)
        typeface = Typeface.create(Typeface.MONOSPACE, Typeface.BOLD)
        letterSpacing = 0.05f
    }
    private val metaPaint = Paint(Paint.ANTI_ALIAS_FLAG).apply {
        color = CarbonPalette.muted
        textSize = sp(9f)
        typeface = Typeface.MONOSPACE
    }
    private var phase = "STANDBY"
    private var workerCount = 0
    private var signalSeed = 0L

    init {
        minimumHeight = (164f * density).toInt()
        contentDescription = "VIC neural trace. Standby. No active Hermes subagents."
    }

    fun setSignal(newPhase: String, runningWorkers: Int, eventId: Long) {
        phase = newPhase.replace('.', ' ').uppercase(Locale.US).take(28)
        workerCount = runningWorkers.coerceAtLeast(0)
        signalSeed = eventId
        contentDescription = "VIC neural trace. $phase. $workerCount active Hermes subagents."
        invalidate()
    }

    override fun onDraw(canvas: Canvas) {
        super.onDraw(canvas)
        val widthValue = width.toFloat()
        val heightValue = height.toFloat()
        val radius = 16f * density
        frame.set(0f, 0f, widthValue, heightValue)
        canvas.drawRoundRect(frame, radius, radius, fillPaint)
        canvas.drawRoundRect(
            RectF(density * 0.5f, density * 0.5f, widthValue - density * 0.5f, heightValue - density * 0.5f),
            radius,
            radius,
            borderPaint,
        )

        clipPath.reset()
        clipPath.addRoundRect(frame, radius, radius, Path.Direction.CW)
        canvas.save()
        canvas.clipPath(clipPath)
        drawGrid(canvas, widthValue, heightValue)
        drawSignal(canvas, widthValue, heightValue)
        canvas.restore()

        val left = 15f * density
        canvas.drawText("VIC // NEURAL TRACE", left, 23f * density, titlePaint)
        canvas.drawText("PHASE  $phase", left, 43f * density, labelPaint)
        canvas.drawText("UPLINK LIVE", left, heightValue - 13f * density, labelPaint)
        val forkText = "FORKS ${workerCount.toString().padStart(2, '0')}"
        canvas.drawText(forkText, widthValue - 15f * density - metaPaint.measureText(forkText), heightValue - 13f * density, metaPaint)

        if (isShown) postInvalidateDelayed(42L)
    }

    private fun drawGrid(canvas: Canvas, widthValue: Float, heightValue: Float) {
        val horizon = heightValue * 0.48f
        repeat(9) { index ->
            val progress = index / 8f
            val y = horizon + ((heightValue - horizon) * progress * progress)
            canvas.drawLine(0f, y, widthValue, y, gridPaint)
        }
        repeat(13) { index ->
            val bottomX = widthValue * index / 12f
            val horizonX = (widthValue * 0.5f) + ((bottomX - widthValue * 0.5f) * 0.18f)
            canvas.drawLine(horizonX, horizon, bottomX, heightValue, gridPaint)
        }
        val travel = ((SystemClock.uptimeMillis() % 2_400L) / 2_400f)
        val scanY = horizon + ((heightValue - horizon) * travel)
        canvas.drawLine(0f, scanY, widthValue, scanY, scanPaint)
    }

    private fun drawSignal(canvas: Canvas, widthValue: Float, heightValue: Float) {
        val baseline = heightValue * 0.57f
        val amplitude = heightValue * 0.09f
        val time = (SystemClock.uptimeMillis() % 4_000L) / 4_000f
        signalPath.reset()
        repeat(49) { index ->
            val progress = index / 48f
            val phaseOffset = (progress * PI * 8.0) + (time * PI * 2.0) + (signalSeed % 17)
            val envelope = if (index % 8 == 0) 1.75 else 0.72
            val x = progress * widthValue
            val y = baseline + (sin(phaseOffset) * amplitude * envelope).toFloat()
            if (index == 0) signalPath.moveTo(x, y) else signalPath.lineTo(x, y)
        }
        canvas.drawPath(signalPath, signalGlowPaint)
        canvas.drawPath(signalPath, signalPaint)
    }

    private fun sp(value: Float): Float = TypedValue.applyDimension(
        TypedValue.COMPLEX_UNIT_SP,
        value,
        resources.displayMetrics,
    )
}
