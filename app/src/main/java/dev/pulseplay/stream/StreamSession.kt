/*
 * PulsePlay — AGPL-3.0-only.
 * Drives a chiaki streaming session against the PS5, keeping the Orbit Play
 * helper's playback lease alive for the duration (so Remote Play stays enabled).
 */
package dev.pulseplay.stream

import android.view.Surface
import com.metallic.chiaki.lib.Codec
import com.metallic.chiaki.lib.ConnectInfo
import com.metallic.chiaki.lib.ConnectVideoProfile
import com.metallic.chiaki.lib.ConnectedEvent
import com.metallic.chiaki.lib.ControllerState
import com.metallic.chiaki.lib.LoginPinRequestEvent
import com.metallic.chiaki.lib.QuitEvent
import com.metallic.chiaki.lib.Session
import com.metallic.chiaki.lib.VideoFPSPreset
import com.metallic.chiaki.lib.VideoResolutionPreset
import dev.pulseplay.net.HelperClient
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.delay
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch

class StreamSession(
    private val console: RegisteredConsole,
    private val onStatus: (String) -> Unit,
    private val onQuit: (String) -> Unit,
) {
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.IO)
    private var session: Session? = null
    @Volatile private var leaseProof: String? = null
    @Volatile private var surface: Surface? = null

    fun start() {
        scope.launch {
            try {
                // Keep Remote Play alive on the console via the helper lease.
                val proof = HelperClient.sessionProof(console.host, console.token)
                leaseProof = proof
                HelperClient.sessionOpen(console.host, console.token, proof)
                launch {
                    while (isActive && leaseProof != null) {
                        delay(10_000)
                        leaseProof?.let {
                            try { HelperClient.sessionOpen(console.host, console.token, it) } catch (_: Exception) {}
                        }
                    }
                }

                val q = StreamPrefs.quality
                val profile = ConnectVideoProfile.preset(q.res, q.fps, q.codec)
                val info = ConnectInfo(console.ps5, console.host, console.registKey, console.morning, profile)
                val s = Session(info, null, false)
                s.eventCallback = { ev ->
                    when (ev) {
                        ConnectedEvent -> onStatus("Connected")
                        is QuitEvent -> onQuit(ev.reasonString ?: "Disconnected")
                        is LoginPinRequestEvent -> onStatus("Login PIN required")
                        else -> {}
                    }
                }
                session = s
                surface?.let { s.setSurface(it) }
                s.start()
                onStatus("Connecting…")
            } catch (e: Exception) {
                onQuit(e.message ?: "Could not start the stream.")
            }
        }
    }

    fun setSurface(sfc: Surface?) {
        surface = sfc
        session?.setSurface(sfc)
    }

    fun setControllerState(state: ControllerState) {
        session?.setControllerState(state)
    }

    fun stop() {
        val p = leaseProof
        leaseProof = null
        scope.launch {
            try { session?.stop() } catch (_: Exception) {}
            try { session?.dispose() } catch (_: Exception) {}
            if (p != null) HelperClient.sessionEnd(console.host, console.token, p)
        }.invokeOnCompletion { scope.cancel() }
    }
}
