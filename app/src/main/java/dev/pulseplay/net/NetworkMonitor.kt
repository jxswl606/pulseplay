/*
 * PulsePlay — AGPL-3.0-only.
 * Watches the phone's active connection (Wi-Fi / mobile data / none) and, with a
 * reachability probe, reports whether the PS5 is on the LAN or reached remotely.
 */
package dev.pulseplay.net

import android.content.Context
import android.net.ConnectivityManager
import android.net.Network
import android.net.NetworkCapabilities
import androidx.compose.runtime.mutableStateOf
import dev.pulseplay.stream.ConsoleStore
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch

enum class NetKind { NONE, WIFI, CELLULAR, ETHERNET }

object NetworkMonitor {
    val kind = mutableStateOf(NetKind.NONE)
    /** True when the saved console's helper answers directly (same local network). */
    val lanReachable = mutableStateOf(false)
    private var cm: ConnectivityManager? = null

    fun init(context: Context) {
        val c = context.applicationContext.getSystemService(Context.CONNECTIVITY_SERVICE) as? ConnectivityManager ?: return
        cm = c
        update()
        runCatching {
            c.registerDefaultNetworkCallback(object : ConnectivityManager.NetworkCallback() {
                override fun onAvailable(n: Network) = update()
                override fun onLost(n: Network) = update()
                override fun onCapabilitiesChanged(n: Network, caps: NetworkCapabilities) = update()
            })
        }
        // Probe LAN reachability of the saved console in the background.
        CoroutineScope(Dispatchers.IO).launch {
            while (isActive) {
                lanReachable.value = probeConsole()
                delay(6000)
            }
        }
    }

    private fun update() {
        val c = cm ?: return
        val caps = runCatching { c.getNetworkCapabilities(c.activeNetwork) }.getOrNull()
        kind.value = when {
            caps == null -> NetKind.NONE
            caps.hasTransport(NetworkCapabilities.TRANSPORT_WIFI) -> NetKind.WIFI
            caps.hasTransport(NetworkCapabilities.TRANSPORT_CELLULAR) -> NetKind.CELLULAR
            caps.hasTransport(NetworkCapabilities.TRANSPORT_ETHERNET) -> NetKind.ETHERNET
            else -> NetKind.NONE
        }
    }

    private fun probeConsole(): Boolean {
        val host = ConsoleStore.console?.host ?: return false
        if (host == "127.0.0.1") return false
        return try {
            java.net.Socket().use { s ->
                s.connect(java.net.InetSocketAddress(host, HelperClient.PORT), 900)
                true
            }
        } catch (_: Exception) { false }
    }

    /** Short connection label for the UI. */
    fun label(): String = when {
        kind.value == NetKind.NONE -> "Not connected"
        kind.value == NetKind.ETHERNET -> "LAN"
        kind.value == NetKind.WIFI && lanReachable.value -> "LAN"
        kind.value == NetKind.WIFI -> "Wi-Fi"
        kind.value == NetKind.CELLULAR -> "Mobile data"
        else -> "Online"
    }

    /** Whether the console can be reached at all right now (LAN or any network for the tunnel). */
    fun online(): Boolean = kind.value != NetKind.NONE

    /** True only when there is no usable connection — the UI shows this red. */
    fun offline(): Boolean = kind.value == NetKind.NONE
}
