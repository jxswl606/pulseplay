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
    private val main = android.os.Handler(android.os.Looper.getMainLooper())
    // @Volatile: written on the IO connect thread, read on the main thread (input/surface).
    @Volatile private var session: Session? = null
    // Gate for controller input: flipped off the instant the stream quits or is
    // stopped, so input never hits a tearing-down native session on the main
    // thread (which froze the UI into an ANR when a controller was active).
    @Volatile private var live = false
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

    /**
     * Open the helper playback lease, retrying when away: a fresh Orbit Link tunnel can
     * take several seconds before it carries the first helper request.
     */
    private suspend fun openLease(console: RegisteredConsole): String {
        val attempts = if (console.host == "127.0.0.1") 15 else 1   // ~15 s of patience over the tunnel
        var last: Exception? = null
        repeat(attempts) { i ->
            try {
                val proof = HelperClient.sessionProof(console.host, console.token)
                HelperClient.sessionOpen(console.host, console.token, proof)
                return proof
            } catch (e: dev.pulseplay.net.HelperError) {
                last = e
                if (e.code != "unreachable" || i == attempts - 1) throw e
                delay(1000)
            }
        }
        throw last ?: dev.pulseplay.net.HelperError("unreachable", "Couldn't reach the PS5.")
    }

    fun start() {
        scope.launch {
            try {
                active = resolveRoute()
                val console = active                 // shadow: rest of start() uses the active route
                // Keep Remote Play alive on the console via the helper lease. Away, the tunnel
                // often needs a few seconds to carry its first packet, so retry (mirrors
                // Orbit Play's _refresh_through_link patience) instead of failing immediately.
                val proof = openLease(console)
                leaseProof = proof
                launch {
                    while (isActive && leaseProof != null) {
                        delay(10_000)
                        leaseProof?.let {
                            try { HelperClient.sessionOpen(console.host, console.token, it) } catch (_: Exception) {}
                        }
                    }
                }

                val r = StreamPrefs.res
                val kbps = if (StreamPrefs.bitrateMbps > 0) StreamPrefs.bitrateMbps * 1000 else r.autoKbps
                val profile = ConnectVideoProfile(r.w, r.h, StreamPrefs.fps.v, kbps, StreamPrefs.codec)
                val info = ConnectInfo(console.ps5, console.host, console.registKey, console.morning, profile)
                val s = Session(info, null, false)
                // chiaki fires events on a native thread. Marshal to the main thread so UI
                // state is never touched off-thread (a crash risk, especially during teardown).
                s.eventCallback = { ev ->
                    main.post {
                        if (stopped) return@post
                        when (ev) {
                            ConnectedEvent -> {
                                live = true
                                onStatus("Connected")
                                // If the user tapped a specific game, boot it now the stream is live.
                                val gid = ActiveStream.pendingGameId
                                val p = leaseProof
                                if (gid != null && p != null) {
                                    ActiveStream.pendingGameId = null
                                    scope.launch { try { HelperClient.launch(console.host, console.token, gid, p) } catch (_: Exception) {} }
                                }
                            }
                            is QuitEvent -> { live = false; onQuit(ev.reasonString ?: "Disconnected") }
                            is LoginPinRequestEvent -> onStatus("Login PIN required")
                            else -> {}
                        }
                    }
                }
                session = s
                // If stop() landed during connect (before the session existed), it couldn't
                // tear this down — so do it here instead of orphaning a live session.
                if (stopped) {
                    session = null
                    try { s.setSurface(null) } catch (_: Throwable) {}
                    try { s.stop() } catch (_: Throwable) {}
                    kotlin.concurrent.thread { try { s.dispose() } catch (_: Throwable) {} }
                    return@launch
                }
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
        if (!live) return            // stream quit/stopped: don't touch the native session
        session?.setControllerState(state)
    }

    @Volatile private var stopped = false

    fun stop() {
        if (stopped) return
        stopped = true
        live = false                 // block any in-flight controller input immediately
        val p = leaseProof
        leaseProof = null
        val s = session
        session = null
        val route = active
        val wasAway = away
        // Main thread, both instant: detach the surface, then signal stop. sessionStop
        // only sets a flag — it does not block.
        try { s?.setSurface(null) } catch (_: Throwable) {}
        try { s?.stop() } catch (_: Throwable) {}
        // dispose() calls sessionJoin, which BLOCKS until the stream threads end — on a
        // dead/slow tunnel that can take seconds. Running it on the main thread froze the
        // UI into an ANR ("hangs then crashes"), so it must go to the background. The
        // surface is already detached and events are main-thread + guarded, so this is safe.
        scope.launch {
            try { s?.dispose() } catch (_: Throwable) {}
            if (p != null) try { HelperClient.sessionEnd(route.host, route.token, p) } catch (_: Exception) {}
            if (wasAway) AwayManager.disconnect()     // bring down the tunnel
        }.invokeOnCompletion { scope.cancel() }
    }
}
