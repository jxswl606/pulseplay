/*
 * PulsePlay — AGPL-3.0-only.
 * Watches the paired console's power state (like Remote Play's Ready / Rest mode /
 * Offline) via chiaki discovery. Unicast to the console's IP so it works over
 * Tailscale as well as the local LAN, not only broadcast.
 */
package dev.pulseplay.stream

import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import com.metallic.chiaki.lib.DiscoveryHost
import com.metallic.chiaki.lib.DiscoveryService
import com.metallic.chiaki.lib.DiscoveryServiceOptions
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch
import java.net.InetSocketAddress

object ConsoleStatus {
    enum class State { CHECKING, ONLINE, REST, OFFLINE }

    // Compose-observable; written from the probe loop, read by the UI.
    var state by mutableStateOf(State.CHECKING)
    var runningApp by mutableStateOf<String?>(null)

    private var job: Job? = null

    /** Start polling the console's state (app-lifetime, like NetworkMonitor). Idempotent. */
    fun start() {
        if (job != null) return
        job = CoroutineScope(Dispatchers.IO).launch {
            while (isActive) {
                val c = ConsoleStore.console
                if (c == null) { state = State.CHECKING; runningApp = null }
                else probe(c.host)
                delay(7000)
            }
        }
    }

    fun label() = when (state) {
        State.ONLINE -> "Ready"
        State.REST -> "Rest mode"
        State.OFFLINE -> "Offline"
        State.CHECKING -> "Checking…"
    }

    /** One unicast discovery to the console; reads its reported power state. */
    private suspend fun probe(host: String) {
        var seen: DiscoveryHost? = null
        var service: DiscoveryService? = null
        try {
            val opts = DiscoveryServiceOptions(4uL, 3uL, 500uL, InetSocketAddress(host, 9302))
            service = DiscoveryService(opts) { hosts ->
                (hosts.firstOrNull { it.hostAddr == host } ?: hosts.firstOrNull())?.let { seen = it }
            }
            val deadline = System.currentTimeMillis() + 2500
            while (seen == null && System.currentTimeMillis() < deadline) delay(150)
        } catch (_: Exception) {
            // discovery blocked / unsupported — treat as offline for this cycle
        } finally {
            runCatching { service?.dispose() }
        }
        val h = seen
        if (h == null) { state = State.OFFLINE; runningApp = null }
        else {
            state = when (h.state) {
                DiscoveryHost.State.READY -> State.ONLINE
                DiscoveryHost.State.STANDBY -> State.REST
                else -> State.OFFLINE
            }
            runningApp = h.runningAppName
        }
    }
}
