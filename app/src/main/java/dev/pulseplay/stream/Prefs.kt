/*
 * PulsePlay — AGPL-3.0-only. Stream quality: independent resolution / frame rate /
 * bandwidth, plus Performance / Balanced / Quality presets. Persisted.
 */
package dev.pulseplay.stream

import android.content.Context
import com.metallic.chiaki.lib.Codec

/** Stream resolutions. w/h are what we request; autoKbps is the default bitrate for "Auto". */
enum class Res(val label: String, val w: Int, val h: Int, val autoKbps: Int) {
    R540("540p", 960, 540, 4000),
    R720("720p", 1280, 720, 10000),
    R1080("1080p", 1920, 1080, 15000),
    R2K("2K", 2560, 1440, 25000),
}

enum class Fps(val label: String, val v: Int) { F30("30", 30), F60("60", 60) }

enum class Preset(val label: String) { PERFORMANCE("Performance"), BALANCED("Balanced"), QUALITY("Quality"), CUSTOM("Custom") }

object StreamPrefs {
    // Defaults = the Performance preset: smooth and low-latency on any network out of the box.
    @Volatile var res: Res = Res.R720
    @Volatile var fps: Fps = Fps.F60
    @Volatile var codec: Codec = Codec.CODEC_H264
    @Volatile var bitrateMbps: Int = 10          // 0 = Auto (per-resolution default)
    @Volatile var alwaysShowControls: Boolean = false
    @Volatile var overlayAlpha: Float = 0.85f
    private var prefs: android.content.SharedPreferences? = null

    val bitrateSteps = listOf(0, 5, 8, 10, 12, 15, 20, 25, 30, 40)     // 0 = Auto (Mbps)
    val opacityOptions = listOf(0.4f, 0.55f, 0.7f, 0.85f, 1.0f)

    fun summary() = "${res.label} · ${fps.label}fps"
    fun bitrateLabel() = if (bitrateMbps == 0) "Auto" else "$bitrateMbps Mbps"
    fun controlsLabel() = if (alwaysShowControls) "Always on-screen" else "Auto (hide with pad)"
    fun opacityLabel() = "${(overlayAlpha * 100).toInt()}%"

    /** The preset matching the current settings, or CUSTOM.
     *  Ladder — all 60fps for responsive input; tuned per goal:
     *   Performance 720p  · H264 · 10 Mbps  (mobile/away: H264 = fast decode, light bandwidth)
     *   Balanced    1080p · H264 · 20 Mbps  (home daily: keeps H264's low-latency smoothness,
     *                                        extra bitrate keeps 1080p clean — the sweet spot)
     *   Quality     1080p · H265 · 30 Mbps  (fast LAN: H265 is more efficient, so the high
     *                                        bitrate buys maximum detail) */
    fun currentPreset(): Preset = when {
        res == Res.R720 && fps == Fps.F60 && codec == Codec.CODEC_H264 && bitrateMbps == 10 -> Preset.PERFORMANCE
        res == Res.R1080 && fps == Fps.F60 && codec == Codec.CODEC_H264 && bitrateMbps == 20 -> Preset.BALANCED
        res == Res.R1080 && fps == Fps.F60 && codec == Codec.CODEC_H265 && bitrateMbps == 30 -> Preset.QUALITY
        else -> Preset.CUSTOM
    }

    fun applyPreset(p: Preset) = when (p) {
        Preset.PERFORMANCE -> setAll(Res.R720, Fps.F60, Codec.CODEC_H264, 10)
        Preset.BALANCED -> setAll(Res.R1080, Fps.F60, Codec.CODEC_H264, 20)
        Preset.QUALITY -> setAll(Res.R1080, Fps.F60, Codec.CODEC_H265, 30)
        Preset.CUSTOM -> {}
    }

    // Named apply* (not setRes/setFps): a `var res` already generates a setRes(Res)
    // JVM method, so a fun setRes(Res) would be a platform declaration clash.
    fun applyRes(r: Res) { res = r; persist() }
    fun applyFps(f: Fps) { fps = f; persist() }
    fun setBitrate(mbps: Int) { bitrateMbps = mbps; persist() }
    /** Step bandwidth up/down through bitrateSteps (both directions). */
    fun stepBitrate(up: Boolean) {
        val i = bitrateSteps.indexOf(bitrateMbps).let { if (it < 0) 0 else it }
        val n = (i + if (up) 1 else -1).coerceIn(0, bitrateSteps.lastIndex)
        bitrateMbps = bitrateSteps[n]; persist()
    }

    private fun setAll(r: Res, f: Fps, c: Codec, b: Int) { res = r; fps = f; codec = c; bitrateMbps = b; persist() }

    fun toggleControls() { alwaysShowControls = !alwaysShowControls; persist() }
    fun cycleOpacity() {
        val i = opacityOptions.indexOfFirst { kotlin.math.abs(it - overlayAlpha) < 0.01f }.let { if (it < 0) 3 else it }
        overlayAlpha = opacityOptions[(i + 1) % opacityOptions.size]; persist()
    }

    fun init(context: Context) {
        val p = context.applicationContext.getSharedPreferences("pulseplay", Context.MODE_PRIVATE)
        prefs = p
        res = runCatching { Res.valueOf(p.getString("res", null) ?: "R720") }.getOrDefault(Res.R720)
        fps = runCatching { Fps.valueOf(p.getString("fps", null) ?: "F60") }.getOrDefault(Fps.F60)
        codec = runCatching { Codec.valueOf(p.getString("codec", null) ?: "CODEC_H264") }.getOrDefault(Codec.CODEC_H264)
        bitrateMbps = p.getInt("bitrate", 10)
        alwaysShowControls = p.getBoolean("alwaysControls", false)
        overlayAlpha = p.getFloat("overlayAlpha", 0.85f)
    }

    private fun persist() {
        prefs?.edit()?.apply {
            putString("res", res.name); putString("fps", fps.name); putString("codec", codec.name)
            putInt("bitrate", bitrateMbps); putBoolean("alwaysControls", alwaysShowControls); putFloat("overlayAlpha", overlayAlpha)
            apply()
        }
    }
}
