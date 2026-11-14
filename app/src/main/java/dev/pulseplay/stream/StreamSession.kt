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
    private val context: android.content.Context,
    private val console: RegisteredConsole,
    private val onStatus: (String) -> Unit,
    private val onQuit: (String) -> Unit,
) {
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.IO)
    private var session: Session? = null
    @Volatile private var leaseProof: String? = null
    @Volatile private var surface: Surface? = null
    // The console actually in use: the saved LAN console at home, or a loopback
    // copy (127.0.0.1 + away key) once Orbit Link is up. Set in start().
    @Volatile private var active: RegisteredConsole = console
    private val away get() = active.host == "127.0.0.1"

    /** Home when the helper answers on the LAN; otherwise bring up Orbit Link. */
    private suspend fun resolveRoute(): RegisteredConsole {
        if (ConsoleStore.link == null) return console          // play-away never set up
        return try {
            HelperClient.sessionProof(console.host, console.token)   // reachable on home Wi-Fi?
            console
        } catch (e: dev.pulseplay.net.HelperError) {
            if (e.code in setOf("unreachable", "protocol", "address")) {
                AwayManager.connectAway(context) { onStatus(it) }
            } else throw e
        }
    }

    fun start() {
        scope.launch {
            try {
                active = resolveRoute()
                val console = active                 // shadow: rest of start() uses the active route
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
                val profile = if (StreamPrefs.bitrateMbps > 0) {
                    val (w, h) = when (q.res) {
                        VideoResolutionPreset.RES_540P -> 960 to 540
                        VideoResolutionPreset.RES_1080P -> 1920 to 1080
                        else -> 1280 to 720
                    }
                    ConnectVideoProfile(w, h, q.fps.value, StreamPrefs.bitrateMbps * 1000, q.codec)
                } else {
                    ConnectVideoProfile.preset(q.res, q.fps, q.codec)
                }
                val info = ConnectInfo(console.ps5, console.host, console.registKey, console.morning, profile)
                val s = Session(info, null, false)
                s.eventCallback = { ev ->
                    when (ev) {
                        ConnectedEvent -> {
                            onStatus("Connected")
                            // If the user tapped a specific game, boot it now the stream is live.
                            val gid = ActiveStream.pendingGameId
                            val p = leaseProof
                            if (gid != null && p != null) {
                                ActiveStream.pendingGameId = null
                                scope.launch { try { HelperClient.launch(console.host, console.token, gid, p) } catch (_: Exception) {} }
                            }
                        }
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

    @Volatile private var stopped = false

    fun stop() {
        if (stopped) return
        stopped = true
        val p = leaseProof
        leaseProof = null
        val s = session
        session = null
        // Detach the surface immediately so the UI is free; tear down in the background.
        try { s?.setSurface(null) } catch (_: Exception) {}
        val route = active
        val wasAway = away
        scope.launch {
            try { s?.stop() } catch (_: Exception) {}
            try { s?.dispose() } catch (_: Exception) {}
            if (p != null) HelperClient.sessionEnd(route.host, route.token, p)
            if (wasAway) AwayManager.disconnect()     // bring down the tunnel
        }.invokeOnCompletion { scope.cancel() }
    }
}
