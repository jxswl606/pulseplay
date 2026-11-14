/*
 * PulsePlay — AGPL-3.0-only.
 * Shared live controller state fed by both the on-screen pad and a physical
 * Bluetooth/USB gamepad, pushed to the active chiaki session.
 */
package dev.pulseplay.stream

import android.view.InputDevice
import android.view.KeyEvent
import android.view.MotionEvent
import com.metallic.chiaki.lib.ControllerState

object ActiveStream {
    @Volatile var session: StreamSession? = null
    // Compose-observable so the on-screen pad hides live when a controller connects.
    val hasGamepadState = androidx.compose.runtime.mutableStateOf(false)
    var hasGamepad: Boolean
        get() = hasGamepadState.value
        set(v) { hasGamepadState.value = v }
    val state = ControllerState()

    private fun push() { session?.setControllerState(state) }
    private fun axis(v: Float) = (v.coerceIn(-1f, 1f) * 32767).toInt().toShort()

    fun button(mask: UInt, down: Boolean) {
        state.buttons = if (down) state.buttons or mask else state.buttons and mask.inv()
        push()
    }
    fun leftStick(x: Float, y: Float) { state.leftX = axis(x); state.leftY = axis(y); push() }
    fun rightStick(x: Float, y: Float) { state.rightX = axis(x); state.rightY = axis(y); push() }
    fun trigger(left: Boolean, f: Float) {
        val v = (f.coerceIn(0f, 1f) * 255).toInt().toUByte()
        if (left) state.l2State = v else state.r2State = v
        push()
    }

    fun clear() { session = null }

    // ---------- physical controller ----------
    private fun keyMask(keyCode: Int): UInt? = when (keyCode) {
        KeyEvent.KEYCODE_BUTTON_A -> ControllerState.BUTTON_CROSS
        KeyEvent.KEYCODE_BUTTON_B -> ControllerState.BUTTON_MOON
        KeyEvent.KEYCODE_BUTTON_X -> ControllerState.BUTTON_BOX
        KeyEvent.KEYCODE_BUTTON_Y -> ControllerState.BUTTON_PYRAMID
        KeyEvent.KEYCODE_BUTTON_L1 -> ControllerState.BUTTON_L1
        KeyEvent.KEYCODE_BUTTON_R1 -> ControllerState.BUTTON_R1
        KeyEvent.KEYCODE_BUTTON_THUMBL -> ControllerState.BUTTON_L3
        KeyEvent.KEYCODE_BUTTON_THUMBR -> ControllerState.BUTTON_R3
        KeyEvent.KEYCODE_BUTTON_START -> ControllerState.BUTTON_OPTIONS
        KeyEvent.KEYCODE_BUTTON_SELECT -> ControllerState.BUTTON_SHARE
        KeyEvent.KEYCODE_BUTTON_MODE -> ControllerState.BUTTON_PS
        KeyEvent.KEYCODE_DPAD_UP -> ControllerState.BUTTON_DPAD_UP
        KeyEvent.KEYCODE_DPAD_DOWN -> ControllerState.BUTTON_DPAD_DOWN
        KeyEvent.KEYCODE_DPAD_LEFT -> ControllerState.BUTTON_DPAD_LEFT
        KeyEvent.KEYCODE_DPAD_RIGHT -> ControllerState.BUTTON_DPAD_RIGHT
        else -> null
    }

    private fun isPad(source: Int) =
        source and InputDevice.SOURCE_GAMEPAD == InputDevice.SOURCE_GAMEPAD ||
        source and InputDevice.SOURCE_JOYSTICK == InputDevice.SOURCE_JOYSTICK

    fun onKey(event: KeyEvent): Boolean {
        if (!isPad(event.source)) return false
        val mask = keyMask(event.keyCode) ?: return false
        when (event.action) {
            KeyEvent.ACTION_DOWN -> { hasGamepad = true; button(mask, true) }
            KeyEvent.ACTION_UP -> button(mask, false)
        }
        return true
    }

    fun onMotion(event: MotionEvent): Boolean {
        if (event.source and InputDevice.SOURCE_JOYSTICK != InputDevice.SOURCE_JOYSTICK) return false
        hasGamepad = true
        state.leftX = axis(event.getAxisValue(MotionEvent.AXIS_X))
        state.leftY = axis(event.getAxisValue(MotionEvent.AXIS_Y))
        state.rightX = axis(event.getAxisValue(MotionEvent.AXIS_Z))
        state.rightY = axis(event.getAxisValue(MotionEvent.AXIS_RZ))
        val lt = event.getAxisValue(MotionEvent.AXIS_LTRIGGER).let { if (it > 0f) it else event.getAxisValue(MotionEvent.AXIS_BRAKE) }
        val rt = event.getAxisValue(MotionEvent.AXIS_RTRIGGER).let { if (it > 0f) it else event.getAxisValue(MotionEvent.AXIS_GAS) }
        state.l2State = (lt.coerceIn(0f, 1f) * 255).toInt().toUByte()
        state.r2State = (rt.coerceIn(0f, 1f) * 255).toInt().toUByte()
        val hx = event.getAxisValue(MotionEvent.AXIS_HAT_X)
        val hy = event.getAxisValue(MotionEvent.AXIS_HAT_Y)
        fun set(mask: UInt, on: Boolean) { state.buttons = if (on) state.buttons or mask else state.buttons and mask.inv() }
        set(ControllerState.BUTTON_DPAD_LEFT, hx < -0.5f); set(ControllerState.BUTTON_DPAD_RIGHT, hx > 0.5f)
        set(ControllerState.BUTTON_DPAD_UP, hy < -0.5f); set(ControllerState.BUTTON_DPAD_DOWN, hy > 0.5f)
        push()
        return true
    }
}
