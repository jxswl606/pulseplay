/*
 * PulsePlay — AGPL-3.0-only.
 * On-screen virtual DualSense in the PS Remote Play style: glassy translucent
 * controls — stacked shoulders/triggers, cross D-pad, double-ring sticks, PS-colored
 * face buttons, and a system row. Each control feeds ActiveStream.
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
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
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

// Frosted-glass tokens: a soft fill, a bright hairline edge, a faint inner ring.
private val Fill = Color(0x24FFFFFF)
private val Edge = Color(0x66FFFFFF)
private val InnerEdge = Color(0x2EFFFFFF)
private val Nub = Color(0x59FFFFFF)
private val Glass = Brush.verticalGradient(listOf(Color(0x2EFFFFFF), Color(0x14FFFFFF)))
// Classic PlayStation face-symbol colours.
private val CrossC = Color(0xFF7FB2FF)
private val MoonC = Color(0xFFFF6B7A)
private val BoxC = Color(0xFFF07FE6)
private val PyramidC = Color(0xFF59E3A0)

// Rounded trapezoids for the shoulder buttons (outer edge taller), like Remote Play.
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
    Box(Modifier.fillMaxSize().alpha(StreamPrefs.overlayAlpha).padding(horizontal = 18.dp, vertical = 14.dp)) {
        // Shoulders / triggers stacked vertically on each side.
        Column(Modifier.align(Alignment.TopStart), horizontalAlignment = Alignment.Start, verticalArrangement = Arrangement.spacedBy(10.dp)) {
            Trapezoid("L2", LeftTrap) { d -> ActiveStream.trigger(true, if (d) 1f else 0f) }
            Trapezoid("L1", LeftTrap) { d -> ActiveStream.button(ControllerState.BUTTON_L1, d) }
        }
        Column(Modifier.align(Alignment.TopEnd), horizontalAlignment = Alignment.End, verticalArrangement = Arrangement.spacedBy(10.dp)) {
            Trapezoid("R2", RightTrap) { d -> ActiveStream.trigger(false, if (d) 1f else 0f) }
            Trapezoid("R1", RightTrap) { d -> ActiveStream.button(ControllerState.BUTTON_R1, d) }
        }

        // Left: D-pad above, stick below.
        Column(Modifier.align(Alignment.BottomStart), horizontalAlignment = Alignment.CenterHorizontally) {
            DPad()
            Spacer(Modifier.size(6.dp))
            Stick { x, y -> ActiveStream.leftStick(x, y) }
        }

        // Right: face buttons above, stick below.
        Column(Modifier.align(Alignment.BottomEnd), horizontalAlignment = Alignment.CenterHorizontally) {
            FaceButtons()
            Spacer(Modifier.size(6.dp))
            Stick { x, y -> ActiveStream.rightStick(x, y) }
        }

        // System row, bottom-center.
        Row(
            Modifier.align(Alignment.BottomCenter).padding(bottom = 8.dp),
            horizontalArrangement = Arrangement.spacedBy(10.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Pill("Create", ControllerState.BUTTON_SHARE)
            Glyph("P", ControllerState.BUTTON_PS)                 // PS button
            Pill("Options", ControllerState.BUTTON_OPTIONS)
            TouchpadButton()
        }
    }
}

private fun pressMod(onDown: () -> Unit, onUp: () -> Unit) = Modifier.pointerInput(Unit) {
    awaitEachGesture {
        awaitFirstDown(requireUnconsumed = false)
        onDown()
        waitForUpOrCancellation()
        onUp()
    }
}

/** A round glassy button with a faint inner ring. */
@Composable
private fun RoundBtn(d: Dp, onPress: (Boolean) -> Unit, content: @Composable () -> Unit) {
    Box(
        Modifier.padding(4.dp).size(d).background(Glass, CircleShape).border(1.dp, Edge, CircleShape)
            .then(pressMod({ onPress(true) }, { onPress(false) })),
        contentAlignment = Alignment.Center,
    ) {
        Box(Modifier.size(d * 0.72f).border(1.dp, InnerEdge, CircleShape))
        content()
    }
}

@Composable
private fun Trapezoid(label: String, shape: Shape, onPress: (Boolean) -> Unit) {
    Box(
        Modifier.size(width = 78.dp, height = 40.dp).background(Glass, shape).border(1.dp, Edge, shape)
            .then(pressMod({ onPress(true) }, { onPress(false) })),
        contentAlignment = Alignment.Center,
    ) { Text(label, color = Color.White, fontSize = 15.sp, fontWeight = FontWeight.Medium) }
}

@Composable
private fun Pill(label: String, mask: UInt) {
    val shape = RoundedCornerShape(50)
    Box(
        Modifier.size(width = 74.dp, height = 36.dp).background(Glass, shape).border(1.dp, Edge, shape)
            .then(pressMod({ ActiveStream.button(mask, true) }, { ActiveStream.button(mask, false) })),
        contentAlignment = Alignment.Center,
    ) { Text(label, color = Color.White, fontSize = 12.sp, fontWeight = FontWeight.Medium) }
}

/** A round system glyph button (PS). */
@Composable
private fun Glyph(label: String, mask: UInt) {
    RoundBtn(48.dp, { d -> ActiveStream.button(mask, d) }) {
        Text(label, color = Color.White, fontSize = 15.sp, fontWeight = FontWeight.Bold)
    }
}

@Composable
private fun FaceButtons() {
    Box(Modifier.size(158.dp)) {
        Box(Modifier.align(Alignment.TopCenter)) { Face("△", ControllerState.BUTTON_PYRAMID, PyramidC) }
        Box(Modifier.align(Alignment.CenterStart)) { Face("□", ControllerState.BUTTON_BOX, BoxC) }
        Box(Modifier.align(Alignment.CenterEnd)) { Face("○", ControllerState.BUTTON_MOON, MoonC) }
        Box(Modifier.align(Alignment.BottomCenter)) { Face("✕", ControllerState.BUTTON_CROSS, CrossC) }
    }
}

@Composable
private fun Face(symbol: String, mask: UInt, color: Color) {
    RoundBtn(50.dp, { d -> ActiveStream.button(mask, d) }) {
        Text(symbol, color = color, fontSize = 19.sp, fontWeight = FontWeight.Bold)
    }
}

@Composable
private fun DPad() {
    val sh = RoundedCornerShape(12.dp)
    Box(Modifier.size(158.dp)) {
        Box(Modifier.align(Alignment.TopCenter)) { DPadKey("▲", ControllerState.BUTTON_DPAD_UP, sh) }
        Box(Modifier.align(Alignment.CenterStart)) { DPadKey("◀", ControllerState.BUTTON_DPAD_LEFT, sh) }
        Box(Modifier.align(Alignment.CenterEnd)) { DPadKey("▶", ControllerState.BUTTON_DPAD_RIGHT, sh) }
        Box(Modifier.align(Alignment.BottomCenter)) { DPadKey("▼", ControllerState.BUTTON_DPAD_DOWN, sh) }
    }
}

@Composable
private fun DPadKey(label: String, mask: UInt, shape: Shape) {
    Box(
        Modifier.padding(4.dp).size(48.dp).background(Glass, shape).border(1.dp, Edge, shape)
            .then(pressMod({ ActiveStream.button(mask, true) }, { ActiveStream.button(mask, false) })),
        contentAlignment = Alignment.Center,
    ) { Text(label, color = Color.White, fontSize = 14.sp) }
}

@Composable
private fun TouchpadButton() {
    val shape = RoundedCornerShape(50)
    Box(
        Modifier.size(width = 56.dp, height = 36.dp).background(Glass, shape).border(1.dp, Edge, shape)
            .then(pressMod({ ActiveStream.button(ControllerState.BUTTON_TOUCHPAD, true) }, { ActiveStream.button(ControllerState.BUTTON_TOUCHPAD, false) })),
        contentAlignment = Alignment.Center,
    ) { Text("⋯⋯", color = Color.White, fontSize = 13.sp) }
}

@Composable
private fun Stick(onMove: (Float, Float) -> Unit) {
    val radiusPx = with(androidx.compose.ui.platform.LocalDensity.current) { 54.dp.toPx() }
    var off by remember { mutableStateOf(Offset.Zero) }
    Box(
        Modifier.padding(6.dp).size(124.dp).background(Glass, CircleShape).border(1.dp, Edge, CircleShape)
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
        Box(Modifier.size(92.dp).border(1.dp, InnerEdge, CircleShape))   // inner ring
        Box(
            Modifier.offset { IntOffset(off.x.roundToInt(), off.y.roundToInt()) }
                .size(50.dp).background(Nub, CircleShape).border(1.dp, Edge, CircleShape),
        )
    }
}
