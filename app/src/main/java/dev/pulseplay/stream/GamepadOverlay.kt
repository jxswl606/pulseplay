/*
 * PulsePlay — AGPL-3.0-only.
 * On-screen virtual DualSense: sticks, D-pad, face buttons, shoulders/triggers,
 * PS/Options/Create and touchpad. Each control feeds ActiveStream.
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
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.IntOffset
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.compose.material3.Text
import com.metallic.chiaki.lib.ControllerState
import kotlin.math.roundToInt
import kotlin.math.sqrt

private val Fill = Color(0x33FFFFFF)
private val Edge = Color(0x55FFFFFF)

@Composable
fun GamepadOverlay() {
    Box(Modifier.fillMaxSize().padding(14.dp)) {
        // shoulders
        Row(Modifier.align(Alignment.TopStart), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            Trigger("L2", left = true); Shoulder("L1", ControllerState.BUTTON_L1)
        }
        Row(Modifier.align(Alignment.TopEnd), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            Shoulder("R1", ControllerState.BUTTON_R1); Trigger("R2", left = false)
        }

        // left: dpad + stick
        Column(Modifier.align(Alignment.BottomStart), horizontalAlignment = Alignment.CenterHorizontally) {
            DPad()
            Spacer(Modifier.size(10.dp))
            Stick { x, y -> ActiveStream.leftStick(x, y) }
            PadButton("L3", ControllerState.BUTTON_L3, 42.dp)
        }

        // right: face buttons + stick
        Column(Modifier.align(Alignment.BottomEnd), horizontalAlignment = Alignment.CenterHorizontally) {
            FaceButtons()
            Spacer(Modifier.size(10.dp))
            Stick { x, y -> ActiveStream.rightStick(x, y) }
            PadButton("R3", ControllerState.BUTTON_R3, 42.dp)
        }

        // center system buttons
        Row(
            Modifier.align(Alignment.BottomCenter).padding(bottom = 6.dp),
            horizontalArrangement = Arrangement.spacedBy(10.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            PadButton("Create", ControllerState.BUTTON_SHARE, 54.dp)
            PadButton("PS", ControllerState.BUTTON_PS, 54.dp)
            PadButton("Options", ControllerState.BUTTON_OPTIONS, 54.dp)
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

@Composable
private fun PadButton(label: String, mask: UInt, d: androidx.compose.ui.unit.Dp, shape: androidx.compose.ui.graphics.Shape = CircleShape) {
    Box(
        Modifier.padding(4.dp).size(d).background(Fill, shape).border(1.dp, Edge, shape)
            .then(pressMod({ ActiveStream.button(mask, true) }, { ActiveStream.button(mask, false) })),
        contentAlignment = Alignment.Center,
    ) { Text(label, color = Color.White, fontSize = 12.sp, fontWeight = FontWeight.SemiBold) }
}

@Composable
private fun Shoulder(label: String, mask: UInt) {
    Box(
        Modifier.size(width = 66.dp, height = 36.dp).background(Fill, RoundedCornerShape(10.dp)).border(1.dp, Edge, RoundedCornerShape(10.dp))
            .then(pressMod({ ActiveStream.button(mask, true) }, { ActiveStream.button(mask, false) })),
        contentAlignment = Alignment.Center,
    ) { Text(label, color = Color.White, fontSize = 13.sp) }
}

@Composable
private fun Trigger(label: String, left: Boolean) {
    Box(
        Modifier.size(width = 66.dp, height = 36.dp).background(Fill, RoundedCornerShape(10.dp)).border(1.dp, Edge, RoundedCornerShape(10.dp))
            .then(pressMod({ ActiveStream.trigger(left, 1f) }, { ActiveStream.trigger(left, 0f) })),
        contentAlignment = Alignment.Center,
    ) { Text(label, color = Color.White, fontSize = 13.sp) }
}

@Composable
private fun FaceButtons() {
    Box(Modifier.size(150.dp)) {
        Box(Modifier.align(Alignment.TopCenter)) { PadButton("△", ControllerState.BUTTON_PYRAMID, 46.dp) }
        Box(Modifier.align(Alignment.CenterStart)) { PadButton("□", ControllerState.BUTTON_BOX, 46.dp) }
        Box(Modifier.align(Alignment.CenterEnd)) { PadButton("○", ControllerState.BUTTON_MOON, 46.dp) }
        Box(Modifier.align(Alignment.BottomCenter)) { PadButton("✕", ControllerState.BUTTON_CROSS, 46.dp) }
    }
}

@Composable
private fun DPad() {
    Box(Modifier.size(150.dp)) {
        Box(Modifier.align(Alignment.TopCenter)) { PadButton("▲", ControllerState.BUTTON_DPAD_UP, 46.dp, RoundedCornerShape(10.dp)) }
        Box(Modifier.align(Alignment.CenterStart)) { PadButton("◀", ControllerState.BUTTON_DPAD_LEFT, 46.dp, RoundedCornerShape(10.dp)) }
        Box(Modifier.align(Alignment.CenterEnd)) { PadButton("▶", ControllerState.BUTTON_DPAD_RIGHT, 46.dp, RoundedCornerShape(10.dp)) }
        Box(Modifier.align(Alignment.BottomCenter)) { PadButton("▼", ControllerState.BUTTON_DPAD_DOWN, 46.dp, RoundedCornerShape(10.dp)) }
    }
}

@Composable
private fun TouchpadButton() {
    Box(
        Modifier.size(width = 90.dp, height = 44.dp).background(Fill, RoundedCornerShape(10.dp)).border(1.dp, Edge, RoundedCornerShape(10.dp))
            .then(pressMod({ ActiveStream.button(ControllerState.BUTTON_TOUCHPAD, true) }, { ActiveStream.button(ControllerState.BUTTON_TOUCHPAD, false) })),
        contentAlignment = Alignment.Center,
    ) { Text("Touchpad", color = Color.White, fontSize = 11.sp) }
}

@Composable
private fun Stick(onMove: (Float, Float) -> Unit) {
    val radiusPx = with(androidx.compose.ui.platform.LocalDensity.current) { 52.dp.toPx() }
    var off by remember { mutableStateOf(Offset.Zero) }
    Box(
        Modifier.padding(6.dp).size(120.dp).background(Fill, CircleShape).border(1.dp, Edge, CircleShape)
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
        Box(
            Modifier.offset { IntOffset(off.x.roundToInt(), off.y.roundToInt()) }
                .size(48.dp).background(Color(0x88FFFFFF), CircleShape),
        )
    }
}
