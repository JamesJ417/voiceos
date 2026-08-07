package dev.voiceos.client

import android.content.Context
import android.graphics.Canvas
import android.graphics.Color
import android.graphics.ColorFilter
import android.graphics.Paint
import android.graphics.Path
import android.graphics.PixelFormat
import android.graphics.RectF
import android.graphics.LinearGradient
import android.graphics.Shader
import android.graphics.Typeface
import android.graphics.drawable.Drawable
import android.graphics.drawable.GradientDrawable
import android.view.View
import android.view.ViewGroup
import android.widget.LinearLayout
import android.widget.TextView
import java.util.Locale
import kotlin.math.cos
import kotlin.math.sin

object CarbonPalette {
    val black = Color.rgb(6, 8, 9)
    val carbon = Color.rgb(11, 14, 16)
    val surface = Color.rgb(18, 23, 26)
    val surfaceRaised = Color.rgb(24, 30, 34)
    val line = Color.rgb(42, 52, 56)
    val teal = Color.rgb(67, 230, 201)
    val cyan = Color.rgb(69, 207, 228)
    val green = Color.rgb(74, 222, 128)
    val amber = Color.rgb(245, 185, 66)
    val purple = Color.rgb(157, 140, 255)
    val red = Color.rgb(239, 107, 115)
    val white = Color.rgb(241, 246, 245)
    val muted = Color.rgb(141, 155, 157)
}

fun Context.carbonPanelLayout(padding: Int = 20): LinearLayout = LinearLayout(this).apply {
    orientation = LinearLayout.VERTICAL
    val pixels = (padding * resources.displayMetrics.density).toInt()
    setPadding(pixels, pixels, pixels, pixels)
    background = carbonPanel(this@carbonPanelLayout)
}

fun Context.carbonKicker(value: String): TextView = TextView(this).apply {
    text = value.uppercase(Locale.US)
    textSize = 10f
    typeface = Typeface.DEFAULT_BOLD
    letterSpacing = 0.17f
    setTextColor(CarbonPalette.teal)
}

fun Context.carbonHeading(value: String, size: Float = 24f): TextView = TextView(this).apply {
    text = value
    textSize = size
    typeface = Typeface.create("sans-serif", Typeface.NORMAL)
    setTextColor(CarbonPalette.white)
}

fun fullWidthWrapLayout(): LinearLayout.LayoutParams = LinearLayout.LayoutParams(
    ViewGroup.LayoutParams.MATCH_PARENT,
    ViewGroup.LayoutParams.WRAP_CONTENT,
)

class CarbonBackgroundDrawable(context: Context) : Drawable() {
    private val density = context.resources.displayMetrics.density
    private val fill = Paint(Paint.ANTI_ALIAS_FLAG)
    private val weaveDark = Paint(Paint.ANTI_ALIAS_FLAG).apply {
        color = Color.argb(24, 255, 255, 255)
        strokeWidth = density * 0.45f
    }
    private val weaveTeal = Paint(Paint.ANTI_ALIAS_FLAG).apply {
        color = Color.argb(24, 67, 230, 201)
        strokeWidth = density * 0.55f
    }
    private val hexPaint = Paint(Paint.ANTI_ALIAS_FLAG).apply {
        color = Color.argb(30, 96, 126, 126)
        style = Paint.Style.STROKE
        strokeWidth = density * 0.7f
    }
    private val nodePaint = Paint(Paint.ANTI_ALIAS_FLAG).apply {
        color = Color.argb(38, 67, 230, 201)
        style = Paint.Style.FILL
    }

    override fun draw(canvas: Canvas) {
        fill.shader = LinearGradient(
            bounds.left.toFloat(), bounds.top.toFloat(),
            bounds.right.toFloat(), bounds.bottom.toFloat(),
            intArrayOf(Color.rgb(24, 32, 35), CarbonPalette.black, Color.rgb(7, 13, 14)),
            floatArrayOf(0f, 0.48f, 1f),
            Shader.TileMode.CLAMP,
        )
        canvas.drawRect(bounds, fill)
        val weaveStep = density * 11f
        var diagonal = -bounds.height().toFloat()
        while (diagonal < bounds.width() + bounds.height()) {
            canvas.drawLine(diagonal, bounds.height().toFloat(), diagonal + bounds.height(), 0f, weaveDark)
            canvas.drawLine(diagonal + weaveStep * 0.45f, 0f, diagonal + bounds.height() + weaveStep * 0.45f, bounds.height().toFloat(), weaveTeal)
            diagonal += weaveStep
        }

        val radius = density * 31f
        val hexWidth = radius * 1.732f
        val rowHeight = radius * 1.5f
        var row = -1
        var centerY = -radius
        while (centerY < bounds.height() + radius) {
            var centerX = if (row % 2 == 0) 0f else hexWidth / 2f
            var column = 0
            while (centerX < bounds.width() + radius) {
                canvas.drawPath(hexPath(centerX, centerY, radius), hexPaint)
                if ((row + column) % 4 == 0) {
                    val angle = Math.toRadians(-30.0)
                    canvas.drawCircle(
                        centerX + radius * cos(angle).toFloat(),
                        centerY + radius * sin(angle).toFloat(),
                        density * 1.35f,
                        nodePaint,
                    )
                }
                centerX += hexWidth
                column += 1
            }
            row += 1
            centerY += rowHeight
        }
    }

    private fun hexPath(centerX: Float, centerY: Float, radius: Float): Path = Path().apply {
        repeat(6) { index ->
            val angle = Math.toRadians((60.0 * index) - 30.0)
            val x = centerX + radius * cos(angle).toFloat()
            val y = centerY + radius * sin(angle).toFloat()
            if (index == 0) moveTo(x, y) else lineTo(x, y)
        }
        close()
    }

    override fun setAlpha(alpha: Int) = Unit
    override fun setColorFilter(colorFilter: ColorFilter?) = Unit
    @Deprecated("Deprecated in Java")
    override fun getOpacity(): Int = PixelFormat.OPAQUE
}

class HexMarkView(context: Context) : View(context) {
    private val outer = Paint(Paint.ANTI_ALIAS_FLAG).apply {
        color = CarbonPalette.teal
        style = Paint.Style.STROKE
        strokeWidth = resources.displayMetrics.density * 2f
    }
    private val inner = Paint(Paint.ANTI_ALIAS_FLAG).apply {
        color = Color.argb(120, 53, 224, 193)
        style = Paint.Style.STROKE
        strokeWidth = resources.displayMetrics.density
    }

    override fun onDraw(canvas: Canvas) {
        super.onDraw(canvas)
        val radius = minOf(width, height) * 0.42f
        drawHex(canvas, width / 2f, height / 2f, radius, outer)
        drawHex(canvas, width / 2f, height / 2f, radius * 0.68f, inner)
    }
}

class HexTalkButton(context: Context) : View(context) {
    private val density = resources.displayMetrics.density
    private val outerPaint = Paint(Paint.ANTI_ALIAS_FLAG).apply { color = Color.rgb(33, 41, 45) }
    private val innerPaint = Paint(Paint.ANTI_ALIAS_FLAG).apply { color = CarbonPalette.surface }
    private val borderPaint = Paint(Paint.ANTI_ALIAS_FLAG).apply {
        color = CarbonPalette.teal
        style = Paint.Style.STROKE
        strokeWidth = density * 3f
    }
    private val ringPaint = Paint(Paint.ANTI_ALIAS_FLAG).apply {
        color = Color.argb(72, 53, 224, 193)
        style = Paint.Style.STROKE
        strokeWidth = density
    }
    private val faintRingPaint = Paint(Paint.ANTI_ALIAS_FLAG).apply {
        color = Color.argb(30, 53, 224, 193)
        style = Paint.Style.STROKE
        strokeWidth = density
    }
    private val micPaint = Paint(Paint.ANTI_ALIAS_FLAG).apply {
        color = CarbonPalette.teal
        style = Paint.Style.STROKE
        strokeCap = Paint.Cap.ROUND
        strokeWidth = density * 5f
    }
    private val textPaint = Paint(Paint.ANTI_ALIAS_FLAG).apply {
        color = CarbonPalette.teal
        textAlign = Paint.Align.CENTER
        typeface = Typeface.create(Typeface.DEFAULT, Typeface.BOLD)
        textSize = density * 21f
        letterSpacing = 0.12f
    }
    private val detailPaint = Paint(Paint.ANTI_ALIAS_FLAG).apply {
        color = CarbonPalette.muted
        textAlign = Paint.Align.CENTER
        textSize = density * 11f
    }
    private var label = "TALK"
    private var detail = "Touch to begin"

    init {
        isClickable = true
        isFocusable = true
        contentDescription = "Talk. Touch to begin."
        setLayerType(LAYER_TYPE_SOFTWARE, null)
    }

    fun setVoiceState(newLabel: String, newDetail: String, accent: Int) {
        label = newLabel
        detail = newDetail
        borderPaint.color = accent
        micPaint.color = accent
        textPaint.color = accent
        ringPaint.color = Color.argb(72, Color.red(accent), Color.green(accent), Color.blue(accent))
        faintRingPaint.color = Color.argb(30, Color.red(accent), Color.green(accent), Color.blue(accent))
        contentDescription = "$newLabel. $newDetail."
        invalidate()
    }

    override fun onDraw(canvas: Canvas) {
        super.onDraw(canvas)
        val centerX = width / 2f
        val centerY = height / 2f
        val radius = minOf(width * 0.36f, height * 0.38f)
        canvas.drawPath(hexPath(centerX, centerY, radius * 1.28f), faintRingPaint)
        canvas.drawPath(hexPath(centerX, centerY, radius * 1.13f), ringPaint)
        outerPaint.setShadowLayer(density * 18f, 0f, density * 9f, Color.argb(210, 0, 0, 0))
        canvas.drawPath(hexPath(centerX, centerY, radius), outerPaint)
        outerPaint.clearShadowLayer()
        canvas.drawPath(hexPath(centerX, centerY, radius * 0.91f), innerPaint)
        borderPaint.setShadowLayer(density * 8f, 0f, 0f, Color.argb(120, Color.red(borderPaint.color), Color.green(borderPaint.color), Color.blue(borderPaint.color)))
        canvas.drawPath(hexPath(centerX, centerY, radius * 0.86f), borderPaint)
        borderPaint.clearShadowLayer()

        val micTop = centerY - density * 61f
        val micRect = RectF(centerX - density * 16f, micTop, centerX + density * 16f, micTop + density * 48f)
        micPaint.style = Paint.Style.FILL
        canvas.drawRoundRect(micRect, density * 16f, density * 16f, micPaint)
        micPaint.style = Paint.Style.STROKE
        canvas.drawArc(RectF(centerX - density * 29f, micTop + density * 18f, centerX + density * 29f, micTop + density * 72f), 0f, 180f, false, micPaint)
        canvas.drawLine(centerX, micTop + density * 72f, centerX, micTop + density * 87f, micPaint)
        canvas.drawLine(centerX - density * 18f, micTop + density * 87f, centerX + density * 18f, micTop + density * 87f, micPaint)
        canvas.drawText(label, centerX, centerY + density * 60f, textPaint)
        canvas.drawText(detail, centerX, centerY + density * 83f, detailPaint)
    }

    override fun performClick(): Boolean {
        super.performClick()
        return true
    }

    private fun hexPath(centerX: Float, centerY: Float, radius: Float): Path = Path().apply {
        repeat(6) { index ->
            val angle = Math.toRadians((60.0 * index) - 30.0)
            val x = centerX + radius * cos(angle).toFloat()
            val y = centerY + radius * sin(angle).toFloat()
            if (index == 0) moveTo(x, y) else lineTo(x, y)
        }
        close()
    }
}

fun carbonPanel(context: Context, strokeColor: Int = CarbonPalette.line): Drawable =
    GradientDrawable(
        GradientDrawable.Orientation.TL_BR,
        intArrayOf(CarbonPalette.surfaceRaised, CarbonPalette.surface),
    ).apply {
        cornerRadius = context.resources.displayMetrics.density * 20f
        setStroke((context.resources.displayMetrics.density).toInt().coerceAtLeast(1), strokeColor)
    }

fun carbonControl(context: Context, accent: Int, filled: Boolean = false): Drawable =
    GradientDrawable(
        GradientDrawable.Orientation.TL_BR,
        if (filled) intArrayOf(accent, darken(accent)) else intArrayOf(CarbonPalette.surfaceRaised, CarbonPalette.surface),
    ).apply {
        cornerRadius = context.resources.displayMetrics.density * 14f
        setStroke((context.resources.displayMetrics.density).toInt().coerceAtLeast(1), if (filled) darken(accent) else CarbonPalette.line)
    }

private fun drawHex(canvas: Canvas, centerX: Float, centerY: Float, radius: Float, paint: Paint) {
    val path = Path()
    repeat(6) { index ->
        val angle = Math.toRadians((60.0 * index) - 30.0)
        val x = centerX + radius * cos(angle).toFloat()
        val y = centerY + radius * sin(angle).toFloat()
        if (index == 0) path.moveTo(x, y) else path.lineTo(x, y)
    }
    path.close()
    canvas.drawPath(path, paint)
}

private fun darken(color: Int): Int = Color.rgb(
    (Color.red(color) * 0.48f).toInt(),
    (Color.green(color) * 0.48f).toInt(),
    (Color.blue(color) * 0.48f).toInt(),
)
