/*
 * PulsePlay — AGPL-3.0-only. Best-effort LAN discovery to auto-fill the PS5 IP.
 */
package dev.pulseplay.stream

import com.metallic.chiaki.lib.DiscoveryService
import com.metallic.chiaki.lib.DiscoveryServiceOptions
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.withContext
import java.net.InetSocketAddress

object Discovery {
    /** Returns the first PS5 IP found on the LAN, or null. Never throws. */
    suspend fun findPs5(): String? = withContext(Dispatchers.IO) {
        var found: String? = null
        var service: DiscoveryService? = null
        try {
            val options = DiscoveryServiceOptions(4uL, 3uL, 500uL, InetSocketAddress("255.255.255.255", 9302))
            service = DiscoveryService(options) { hosts ->
                if (found == null) found = hosts.firstOrNull { it.hostAddr != null }?.hostAddr
            }
            val deadline = System.currentTimeMillis() + 4500
            while (found == null && System.currentTimeMillis() < deadline) delay(200)
        } catch (_: Exception) {
            // discovery unsupported / blocked — fall back to manual entry
        } finally {
            runCatching { service?.dispose() }
        }
        found
    }
}
