/*
 * PulsePlay — AGPL-3.0-only.
 * On-screen virtual DualSense, laid out responsively: every control scales to the
 * screen (BoxWithConstraints) and spreads to the edges, so it fits any phone with
 * no overlap. Sticks at the outer bottom corners, D-pad / face buttons inner,
 * shoulders top corners, small system glyphs bottom-center. Each control feeds
 * ActiveStream.
 */
package dev.pulseplay.stream

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.foundation.gestures.detectDragGestures
import androidx.compose.foundation.gestures.waitForUpOrCancellation
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.GenericShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Shape
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.IntOffset
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.compose.material3.Text
import com.metallic.chiaki.lib.ControllerState
import kotlin.math.roundToInt
import kotlin.math.sqrt

// Frosted-glass tokens.
private val Edge = Color(0x66FFFFFF)
private val InnerEdge = Color(0x2EFFFFFF)
private val Nub = Color(0x59FFFFFF)
private val Glass = Brush.verticalGradient(listOf(Color(0x2EFFFFFF), Color(0x14FFFFFF)))
// Classic PlayStation face-symbol colours.
private val CrossC = Color(0xFF7FB2FF)
private val MoonC = Color(0xFFFF6B7A)
private val BoxC = Color(0xFFF07FE6)
private val PyramidC = Color(0xFF59E3A0)

// Rounded trapezoids for the shoulder buttons (outer edge taller).
private val LeftTrap = GenericShape { s, _ ->
    moveTo(s.width * 0.06f, 0f); lineTo(s.width, s.height * 0.16f)
    lineTo(s.width, s.height * 0.84f); lineTo(s.width * 0.06f, s.height); close()
}
private val RightTrap = GenericShape { s, _ ->
    moveTo(0f, s.height * 0.16f); lineTo(s.width * 0.94f, 0f)
    lineTo(s.width * 0.94f, s.height); lineTo(0f, s.height * 0.84f); close()
}

@Composable
fun GamepadOverlay() {
    val dyn = StreamPrefs.dynamicSticks
    Box(Modifier.fillMaxSize()) {
        // Floating sticks: a full-screen, unpadded layer UNDER the buttons. Buttons
        // consume their own touches (see pressMod), so everything left/right of them
        // spawns a stick under the thumb.
        if (dyn) FloatingSticks()

        BoxWithConstraints(Modifier.fillMaxSize().alpha(StreamPrefs.overlayAlpha).padding(horizontal = 14.dp, vertical = 12.dp)) {
            // Scale every control to the screen height (360dp baseline), clamped sane.
            val scale = (maxHeight / 360.dp).coerceIn(0.68f, 1.3f)

            // Shoulders: top corners, stacked, sitting clear below the status chip.
            Column(Modifier.align(Alignment.TopStart).padding(top = 42.dp), verticalArrangement = Arrangement.spacedBy(8.dp * scale)) {
                Trapezoid("L2", LeftTrap, scale) { d -> ActiveStream.trigger(true, if (d) 1f else 0f) }
                Trapezoid("L1", LeftTrap, scale) { d -> ActiveStream.button(ControllerState.BUTTON_L1, d) }
            }
            Column(Modifier.align(Alignment.TopEnd).padding(top = 42.dp), horizontalAlignment = Alignment.End, verticalArrangement = Arrangement.spacedBy(8.dp * scale)) {
                Trapezoid("R2", RightTrap, scale) { d -> ActiveStream.trigger(false, if (d) 1f else 0f) }
                Trapezoid("R1", RightTrap, scale) { d -> ActiveStream.button(ControllerState.BUTTON_R1, d) }
            }

            // Bottom-left: D-pad, with L3 inboard, plus the fixed left stick only when not floating.
            Row(Modifier.align(Alignment.BottomStart), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(10.dp * scale)) {
                DPad(scale)
                StickClickBtn("L3", ControllerState.BUTTON_L3, scale)
                if (!dyn) Stick(scale) { x, y -> ActiveStream.leftStick(x, y) }
            }

            // Bottom-right: face buttons, with R3 inboard, plus the fixed right stick only when not floating.
            Row(Modifier.align(Alignment.BottomEnd), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(10.dp * scale)) {
                if (!dyn) Stick(scale) { x, y -> ActiveStream.rightStick(x, y) }
                StickClickBtn("R3", ControllerState.BUTTON_R3, scale)
                FaceButtons(scale)
            }

            // System buttons: small, translucent, bottom-center between the thumb clusters.
            Row(
                Modifier.align(Alignment.BottomCenter).padding(bottom = 12.dp).alpha(0.78f),
                horizontalArrangement = Arrangement.spacedBy(8.dp * scale),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                SysBtn(SysGlyph.CREATE, ControllerState.BUTTON_SHARE, scale)   // Create / screenshot
                SysBtn(SysGlyph.HOME, ControllerState.BUTTON_PS, scale)        // PS / Home
                SysBtn(SysGlyph.OPTIONS, ControllerState.BUTTON_OPTIONS, scale) // Options / Start
            }
        }
    }
}

/**
 * Remote Play-style floating sticks. The left ~42% of the screen drives the left
 * stick, the right ~42% the right stick; touch spawns the stick under your thumb,
 * drag steers, release clears it. Button touches (consumed by the buttons drawn
 * above this layer) are ignored. Multi-touch: both sticks work at once.
 */
@Composable
private fun FloatingSticks() {
    var leftOrigin by remember { mutableStateOf<Offset?>(null) }
    var leftPos by remember { mutableStateOf(Offset.Zero) }
    var rightOrigin by remember { mutableStateOf<Offset?>(null) }
    var rightPos by remember { mutableStateOf(Offset.Zero) }

    Box(Modifier.fillMaxSize().pointerInput(Unit) {
        val radiusPx = 56.dp.toPx()
        // Three zones, like the sticks you asked for: left 40% = left stick, right 40% =
        // right stick, middle 20% = touchpad (swipe = touchpad swipe, e.g. the wind).
        val leftEdge = size.width * 0.40f
        val rightEdge = size.width * 0.60f
        val h = size.height.toFloat()
        var leftId: androidx.compose.ui.input.pointer.PointerId? = null
        var rightId: androidx.compose.ui.input.pointer.PointerId? = null
        var centerId: androidx.compose.ui.input.pointer.PointerId? = null
        var tpId = -1                       // chiaki touchpad touch id for the active swipe
        var centerStart = Offset.Zero
        var centerMoved = false

        fun steer(origin: Offset, p: Offset): Offset {
            var d = p - origin
            val len = sqrt(d.x * d.x + d.y * d.y)
            if (len > radiusPx) d *= (radiusPx / len)
            return d
        }
        // Normalize a point inside the center zone to 0..1 across the DualSense touchpad.
        fun nx(p: Offset) = ((p.x - leftEdge) / (rightEdge - leftEdge)).coerceIn(0f, 1f)
        fun ny(p: Offset) = (p.y / h).coerceIn(0f, 1f)

        awaitPointerEventScope {
            while (true) {
                val event = awaitPointerEvent()
                for (c in event.changes) {
                    val down = c.pressed && !c.previousPressed
                    val up = !c.pressed && c.previousPressed
                    when {
                        down && !c.isConsumed && c.position.x < leftEdge && leftId == null -> {
                            leftId = c.id; leftOrigin = c.position; leftPos = c.position
                            ActiveStream.leftStick(0f, 0f); c.consume()
                        }
                        down && !c.isConsumed && c.position.x > rightEdge && rightId == null -> {
                            rightId = c.id; rightOrigin = c.position; rightPos = c.position
                            ActiveStream.rightStick(0f, 0f); c.consume()
                        }
                        down && !c.isConsumed && c.position.x >= leftEdge && c.position.x <= rightEdge && centerId == null -> {
                            centerId = c.id; centerStart = c.position; centerMoved = false
                            tpId = ActiveStream.touchStart(nx(c.position), ny(c.position)); c.consume()
                        }
                        up && c.id == leftId -> { leftId = null; leftOrigin = null; ActiveStream.leftStick(0f, 0f); c.consume() }
                        up && c.id == rightId -> { rightId = null; rightOrigin = null; ActiveStream.rightStick(0f, 0f); c.consume() }
                        up && c.id == centerId -> {
                            ActiveStream.touchEnd(tpId)
                            // A tap that never really moved = a touchpad click (map/menu).
                            if (!centerMoved) { ActiveStream.button(ControllerState.BUTTON_TOUCHPAD, true); ActiveStream.button(ControllerState.BUTTON_TOUCHPAD, false) }
                            centerId = null; tpId = -1; c.consume()
                        }
                        c.pressed && c.id == leftId -> {
                            val o = leftOrigin ?: c.position
                            val d = steer(o, c.position); leftPos = o + d
                            ActiveStream.leftStick(d.x / radiusPx, d.y / radiusPx); c.consume()
                        }
                        c.pressed && c.id == rightId -> {
                            val o = rightOrigin ?: c.position
                            val d = steer(o, c.position); rightPos = o + d
                            ActiveStream.rightStick(d.x / radiusPx, d.y / radiusPx); c.consume()
                        }
                        c.pressed && c.id == centerId -> {
                            if ((c.position - centerStart).getDistance() > 14f) centerMoved = true
                            ActiveStream.touchMove(tpId, nx(c.position), ny(c.position)); c.consume()
                        }
                    }
                }
            }
        }
    }) {
        androidx.compose.foundation.Canvas(Modifier.fillMaxSize()) {
            val r = 56.dp.toPx()
            leftOrigin?.let { o -> drawFloatingStick(o, leftPos, r) }
            rightOrigin?.let { o -> drawFloatingStick(o, rightPos, r) }
        }
    }
}

private fun androidx.compose.ui.graphics.drawscope.DrawScope.drawFloatingStick(origin: Offset, pos: Offset, r: Float) {
    drawCircle(Color(0x22FFFFFF), r, origin)                                   // base
    drawCircle(Color(0x59FFFFFF), r, origin, style = androidx.compose.ui.graphics.drawscope.Stroke(width = r * 0.045f))  // ring
    drawCircle(Color(0x8CFFFFFF), r * 0.46f, pos)                             // thumb nub
}

private fun pressMod(onDown: () -> Unit, onUp: () -> Unit) = Modifier.pointerInput(Unit) {
    awaitEachGesture {
        // Consume the down so the floating-stick layer below ignores button touches.
        awaitFirstDown(requireUnconsumed = false).also { it.consume() }
        onDown()
        waitForUpOrCancellation()
        onUp()
    }
}

@Composable
private fun Trapezoid(label: String, shape: Shape, scale: Float, onPress: (Boolean) -> Unit) {
    Box(
        Modifier.size(width = 70.dp * scale, height = 32.dp * scale).background(Glass, shape).border(1.dp, Edge, shape)
            .then(pressMod({ onPress(true) }, { onPress(false) })),
        contentAlignment = Alignment.Center,
    ) { Text(label, color = Color.White, fontSize = (13 * scale).sp, fontWeight = FontWeight.Medium) }
}

/** A round glassy button with a faint inner ring. */
@Composable
private fun RoundBtn(d: Dp, onPress: (Boolean) -> Unit, content: @Composable () -> Unit) {
    Box(
        Modifier.padding(3.dp).size(d).background(Glass, CircleShape).border(1.dp, Edge, CircleShape)
            .then(pressMod({ onPress(true) }, { onPress(false) })),
        contentAlignment = Alignment.Center,
    ) {
        Box(Modifier.size(d * 0.72f).border(1.dp, InnerEdge, CircleShape))
        content()
    }
}

@Composable
private fun FaceButtons(scale: Float) {
    val d = 34.dp * scale
    Box(Modifier.size(112.dp * scale)) {
        Box(Modifier.align(Alignment.TopCenter)) { Face("△", ControllerState.BUTTON_PYRAMID, PyramidC, d, scale) }
        Box(Modifier.align(Alignment.CenterStart)) { Face("□", ControllerState.BUTTON_BOX, BoxC, d, scale) }
        Box(Modifier.align(Alignment.CenterEnd)) { Face("○", ControllerState.BUTTON_MOON, MoonC, d, scale) }
        Box(Modifier.align(Alignment.BottomCenter)) { Face("✕", ControllerState.BUTTON_CROSS, CrossC, d, scale) }
    }
}

@Composable
private fun Face(symbol: String, mask: UInt, color: Color, d: Dp, scale: Float) {
    RoundBtn(d, { down -> ActiveStream.button(mask, down) }) {
        Text(symbol, color = color, fontSize = (16 * scale).sp, fontWeight = FontWeight.Bold)
    }
}

@Composable
private fun DPad(scale: Float) {
    val sh = RoundedCornerShape(10.dp)
    val key = 32.dp * scale
    Box(Modifier.size(112.dp * scale)) {
        Box(Modifier.align(Alignment.TopCenter)) { DPadKey("▲", ControllerState.BUTTON_DPAD_UP, sh, key, scale) }
        Box(Modifier.align(Alignment.CenterStart)) { DPadKey("◀", ControllerState.BUTTON_DPAD_LEFT, sh, key, scale) }
        Box(Modifier.align(Alignment.CenterEnd)) { DPadKey("▶", ControllerState.BUTTON_DPAD_RIGHT, sh, key, scale) }
        Box(Modifier.align(Alignment.BottomCenter)) { DPadKey("▼", ControllerState.BUTTON_DPAD_DOWN, sh, key, scale) }
    }
}

@Composable
private fun DPadKey(label: String, mask: UInt, shape: Shape, d: Dp, scale: Float) {
    Box(
        Modifier.size(d).background(Glass, shape).border(1.dp, Edge, shape)
            .then(pressMod({ ActiveStream.button(mask, true) }, { ActiveStream.button(mask, false) })),
        contentAlignment = Alignment.Center,
    ) { Text(label, color = Color.White, fontSize = (12 * scale).sp) }
}

@Composable
private fun Stick(scale: Float, onMove: (Float, Float) -> Unit) {
    val d = 104.dp * scale
    val nub = 44.dp * scale
    val radiusPx = with(androidx.compose.ui.platform.LocalDensity.current) { (d * 0.42f).toPx() }
    var off by remember { mutableStateOf(Offset.Zero) }
    Box(
        Modifier.size(d).background(Glass, CircleShape).border(1.dp, Edge, CircleShape)
            .pointerInput(Unit) {
                detectDragGestures(
                    onDrag = { change, drag ->
                        change.consume()
                        var n = off + drag
                        val len = sqrt(n.x * n.x + n.y * n.y)
                        if (len > radiusPx) n *= (radiusPx / len)
                        off = n
                        onMove(n.x / radiusPx, n.y / radiusPx)
                    },
                    onDragEnd = { off = Offset.Zero; onMove(0f, 0f) },
                    onDragCancel = { off = Offset.Zero; onMove(0f, 0f) },
                )
            },
        contentAlignment = Alignment.Center,
    ) {
        Box(Modifier.size(d * 0.76f).border(1.dp, InnerEdge, CircleShape))   // inner ring
        Box(
            Modifier.offset { IntOffset(off.x.roundToInt(), off.y.roundToInt()) }
                .size(nub).background(Nub, CircleShape).border(1.dp, Edge, CircleShape),
        )
    }
}

/** Small labelled pill that clicks a stick in (L3 / R3) — the sticks themselves can't be
 *  "pressed" on a touchscreen, so these give you that button. */
@Composable
private fun StickClickBtn(label: String, mask: UInt, scale: Float) {
    val shape = RoundedCornerShape(50)
    Box(
        Modifier.size(width = 34.dp * scale, height = 28.dp * scale)
            .background(Color(0x1FFFFFFF), shape).border(1.dp, Color(0x3DFFFFFF), shape)
            .then(pressMod({ ActiveStream.button(mask, true) }, { ActiveStream.button(mask, false) })),
        contentAlignment = Alignment.Center,
    ) { Text(label, color = Color.White, fontSize = (11 * scale).sp, fontWeight = FontWeight.Medium) }
}

private enum class SysGlyph { CREATE, HOME, OPTIONS, TOUCHPAD }

/** A small, translucent system button showing a drawn glyph (no text). */
@Composable
private fun SysBtn(glyph: SysGlyph, mask: UInt, scale: Float, wide: Boolean = false) {
    val shape = RoundedCornerShape(50)
    Box(
        Modifier.size(width = (if (wide) 48 else 38).dp * scale, height = 28.dp * scale)
            .background(Color(0x1FFFFFFF), shape).border(1.dp, Color(0x3DFFFFFF), shape)
            .then(pressMod({ ActiveStream.button(mask, true) }, { ActiveStream.button(mask, false) })),
        contentAlignment = Alignment.Center,
    ) { SysIcon(glyph, scale) }
}

@Composable
private fun SysIcon(glyph: SysGlyph, scale: Float) {
    androidx.compose.foundation.Canvas(Modifier.size(16.dp * scale)) {
        val s = size.minDimension
        val st = s * 0.09f
        val cap = androidx.compose.ui.graphics.StrokeCap.Round
        val stroke = androidx.compose.ui.graphics.drawscope.Stroke(width = st, cap = cap, join = androidx.compose.ui.graphics.StrokeJoin.Round)
        fun o(x: Float, y: Float) = androidx.compose.ui.geometry.Offset(s * x, s * y)
        val c = Color.White
        when (glyph) {
            SysGlyph.CREATE -> {   // split capture rectangle
                drawRoundRect(c, topLeft = o(0.14f, 0.3f), size = androidx.compose.ui.geometry.Size(s * 0.72f, s * 0.4f),
                    cornerRadius = androidx.compose.ui.geometry.CornerRadius(s * 0.1f, s * 0.1f), style = stroke)
                drawLine(c, o(0.5f, 0.32f), o(0.5f, 0.68f), strokeWidth = st, cap = cap)
            }
            SysGlyph.HOME -> {     // house
                drawLine(c, o(0.2f, 0.52f), o(0.5f, 0.24f), strokeWidth = st, cap = cap)
                drawLine(c, o(0.5f, 0.24f), o(0.8f, 0.52f), strokeWidth = st, cap = cap)
                drawLine(c, o(0.29f, 0.47f), o(0.29f, 0.76f), strokeWidth = st, cap = cap)
                drawLine(c, o(0.71f, 0.47f), o(0.71f, 0.76f), strokeWidth = st, cap = cap)
                drawLine(c, o(0.29f, 0.76f), o(0.71f, 0.76f), strokeWidth = st, cap = cap)
            }
            SysGlyph.OPTIONS -> {  // three lines
                drawLine(c, o(0.2f, 0.33f), o(0.8f, 0.33f), strokeWidth = st, cap = cap)
                drawLine(c, o(0.2f, 0.5f), o(0.8f, 0.5f), strokeWidth = st, cap = cap)
                drawLine(c, o(0.2f, 0.67f), o(0.8f, 0.67f), strokeWidth = st, cap = cap)
            }
            SysGlyph.TOUCHPAD -> { // the pad's rectangle
                drawRoundRect(c, topLeft = o(0.13f, 0.27f), size = androidx.compose.ui.geometry.Size(s * 0.74f, s * 0.46f),
                    cornerRadius = androidx.compose.ui.geometry.CornerRadius(s * 0.12f, s * 0.12f), style = stroke)
            }
        }
    }
}
