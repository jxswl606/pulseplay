/*
 * PulsePlay — AGPL-3.0-only. Stream quality presets, persisted.
 */
package dev.pulseplay.stream

import android.content.Context
import com.metallic.chiaki.lib.Codec
import com.metallic.chiaki.lib.VideoFPSPreset
import com.metallic.chiaki.lib.VideoResolutionPreset

enum class QualityPreset(
    val label: String,
    val res: VideoResolutionPreset,
    val fps: VideoFPSPreset,
    val codec: Codec,
    val summary: String,
) {
    DATA_SAVER("Data saver", VideoResolutionPreset.RES_540P, VideoFPSPreset.FPS_30, Codec.CODEC_H264, "540p · 30fps"),
    BALANCED("Balanced", VideoResolutionPreset.RES_720P, VideoFPSPreset.FPS_60, Codec.CODEC_H265, "720p · 60fps"),
    PERFORMANCE("Performance", VideoResolutionPreset.RES_1080P, VideoFPSPreset.FPS_60, Codec.CODEC_H265, "1080p · 60fps"),
}

object StreamPrefs {
    @Volatile var quality: QualityPreset = QualityPreset.BALANCED
    @Volatile var bitrateMbps: Int = 0          // 0 = Auto (preset default)
    @Volatile var alwaysShowControls: Boolean = false
    @Volatile var overlayAlpha: Float = 0.85f   // on-screen controls opacity (0.3–1.0)
    private var prefs: android.content.SharedPreferences? = null

    val bitrateOptions = listOf(0, 10, 15, 20, 30)
    val opacityOptions = listOf(0.4f, 0.55f, 0.7f, 0.85f, 1.0f)
    fun bitrateLabel() = if (bitrateMbps == 0) "Auto" else "$bitrateMbps Mbps"
    fun controlsLabel() = if (alwaysShowControls) "Always on-screen" else "Auto (hide with pad)"
    fun opacityLabel() = "${(overlayAlpha * 100).toInt()}%"

    fun init(context: Context) {
        val p = context.applicationContext.getSharedPreferences("pulseplay", Context.MODE_PRIVATE)
        prefs = p
        quality = runCatching { QualityPreset.valueOf(p.getString("quality", null) ?: "BALANCED") }
            .getOrDefault(QualityPreset.BALANCED)
        bitrateMbps = p.getInt("bitrate", 0)
        alwaysShowControls = p.getBoolean("alwaysControls", false)
        overlayAlpha = p.getFloat("overlayAlpha", 0.85f)
    }

    fun cycleOpacity() {
        val i = opacityOptions.indexOfFirst { kotlin.math.abs(it - overlayAlpha) < 0.01f }.let { if (it < 0) 3 else it }
        overlayAlpha = opacityOptions[(i + 1) % opacityOptions.size]
        prefs?.edit()?.putFloat("overlayAlpha", overlayAlpha)?.apply()
    }

    fun set(q: QualityPreset) { quality = q; prefs?.edit()?.putString("quality", q.name)?.apply() }

    fun cycleBitrate() {
        val i = bitrateOptions.indexOf(bitrateMbps).let { if (it < 0) 0 else it }
        bitrateMbps = bitrateOptions[(i + 1) % bitrateOptions.size]
        prefs?.edit()?.putInt("bitrate", bitrateMbps)?.apply()
    }

    fun toggleControls() {
        alwaysShowControls = !alwaysShowControls
        prefs?.edit()?.putBoolean("alwaysControls", alwaysShowControls)?.apply()
    }
}
