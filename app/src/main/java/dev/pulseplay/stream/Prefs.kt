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
    private var prefs: android.content.SharedPreferences? = null

    fun init(context: Context) {
        val p = context.applicationContext.getSharedPreferences("pulseplay", Context.MODE_PRIVATE)
        prefs = p
        quality = runCatching { QualityPreset.valueOf(p.getString("quality", null) ?: "BALANCED") }
            .getOrDefault(QualityPreset.BALANCED)
    }

    fun set(q: QualityPreset) {
        quality = q
        prefs?.edit()?.putString("quality", q.name)?.apply()
    }
}
