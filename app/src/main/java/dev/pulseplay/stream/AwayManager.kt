/*
 * PulsePlay — AGPL-3.0-only.
 * Play away from home, through Orbit Link. Set up once on the home network
 * (link_setup), then connect from anywhere (find_away). Ports Orbit Play's
 * app/orbit_play/service.py link_setup() and find_away()/_link_reach().
 */
package dev.pulseplay.stream

import android.content.Context
import dev.pulseplay.net.HelperClient
import dev.pulseplay.net.HelperError
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.withContext
import org.json.JSONObject

object AwayManager {
    @Volatile private var node: LinkNode? = null

    /** True once play-away has been set up at home and a key is saved. */
    val configured: Boolean get() = ConsoleStore.link != null

    private val LINK_KEY = Regex("[A-Za-z0-9+/]{43}=")
    private val INVITE_CODE = Regex("[A-Za-z0-9-]{4,32}")
    private fun isAwayKey(s: String) = s.length == 64 && s.all { it in "0123456789abcdef" }
    private fun ok(r: JSONObject) = r.optBoolean("ok")

    /**
     * On the home network, with the console already paired: enroll the PS5 and this
     * device in one Orbit Link network and save the pre-authorized away key.
     */
    suspend fun setupAtHome(context: Context, onStatus: (String) -> Unit = {}) = withContext(Dispatchers.IO) {
        val console = ConsoleStore.console
            ?: throw HelperError("state", "Pair with your PS5 on your home Wi-Fi first.")
        val host = console.host
        val token = console.token

        onStatus("Setting up play away from home on your PS5…")
        var state = HelperClient.link("GET", host, token)
        if (!state.optBoolean("configured")) {
            // Empty server => the PS5 uses Orbit Play's public Orbit Link server.
            HelperClient.link("POST", host, token, action = "setup", server = "", network = "Home")
            state = HelperClient.link("GET", host, token)
        }
        val device = state.optJSONObject("device")
            ?: throw HelperError("protocol", "The helper returned an invalid Orbit Link setup.")
        val key = device.optString("key")
        val networkServer = device.optString("server")
        if (!LINK_KEY.matches(key) || networkServer.isEmpty() || networkServer.length > 200) {
            throw HelperError("protocol", "The helper returned an invalid Orbit Link setup.")
        }

        onStatus("Adding this device to the network…")
        val n = LinkNode(context).also { it.start() }
        node = n
        try {
            var status = n.status()
            if (status.optBoolean("configured") && !LinkNode.hasPeer(status, key)) {
                n.act("leave", JSONObject())       // this device belongs elsewhere: move it over
                status = n.status()
            }
            if (!status.optBoolean("configured")) {
                val code = HelperClient.link("POST", host, token, action = "invite").optString("code")
                if (!INVITE_CODE.matches(code)) {
                    throw HelperError("protocol", "The helper returned an invalid Orbit Link code.")
                }
                val r = n.act("setup", JSONObject().put("server", networkServer).put("code", code))
                if (!ok(r)) throw HelperError("link_join", "This device could not join your PS5's Orbit Link network. Check its internet connection.")
            }

            val deadline = System.currentTimeMillis() + 10_000
            while (!LinkNode.hasPeer(n.status(), key)) {
                if (System.currentTimeMillis() >= deadline) {
                    throw HelperError("link_join", "This device joined Orbit Link but does not see the PS5 yet. Try again.")
                }
                delay(300)
            }
            ensureForwards(n, key)

            onStatus("Saving your away key…")
            val awayKey = HelperClient.link("POST", host, token, action = "key").optString("key")
            if (!isAwayKey(awayKey)) {
                throw HelperError("protocol", "The helper returned an invalid key for play away from home.")
            }
            ConsoleStore.saveLink(LinkConfig(host, key, awayKey))
            onStatus("Play away from home is on. Away, keep the PS5 on with the helper running, then just press Play.")
        } finally {
            n.stop()                                // not needed until you actually play away
            node = null
        }
    }

    /**
     * Not on the home network: bring up the tunnel and return a console pointed at
     * 127.0.0.1 (using the away key for the lease). The PS5 was registered at home,
     * so chiaki's keys are unchanged — only the address and token differ.
     */
    suspend fun connectAway(context: Context, onStatus: (String) -> Unit = {}): RegisteredConsole = withContext(Dispatchers.IO) {
        val cfg = ConsoleStore.link
            ?: throw HelperError("state", "Set up play away from home while on your home Wi-Fi first.")
        val base = ConsoleStore.console
            ?: throw HelperError("state", "Pair with your PS5 first.")

        onStatus("Not on your home Wi-Fi. Connecting through Orbit Link…")
        val n = LinkNode(context).also { it.start() }
        node = n
        try {
            reach(n, cfg.key, onStatus)
            // Helper + Remote Play ports are now forwarded to 127.0.0.1 on this device.
            base.copy(host = "127.0.0.1", token = cfg.token)
        } catch (e: Throwable) {
            n.stop(); node = null
            throw e
        }
    }

    /** Tear down the tunnel after an away session ends. */
    fun disconnect() {
        node?.stop()
        node = null
    }

    /** Wait for a working path to the PS5 peer, then forward its ports. Ports _link_reach(). */
    private suspend fun reach(n: LinkNode, key: String, onStatus: (String) -> Unit) {
        val started = System.currentTimeMillis()
        while (true) {
            val status = n.status()
            if (LinkNode.peerReady(status, key)) break           // online + a working path
            val waited = System.currentTimeMillis() - started
            val settled = waited >= 26_000      // server refreshes presence every ~25 s
            val server = status.optJSONObject("server")?.optString("state") ?: ""
            val present = LinkNode.hasPeer(status, key)
            val online = LinkNode.peerOnline(status, key)
            if (server == "removed" || (settled && server == "connected" && !present)) {
                throw HelperError("link_removed", "This device is no longer in your PS5's Orbit Link network. At home, turn play away from home on again.")
            }
            if (settled && server == "connected" && !online) {
                throw HelperError("away_offline", "Your PS5 isn't online for play away from home. Keep it on (not Rest Mode), online, with the helper running.")
            }
            if (waited >= 30_000) {
                if (online) throw HelperError("away_blocked", "Orbit Link sees your PS5 but couldn't connect to it directly. This network or your home router blocks direct connections; try another network.")
                throw HelperError("away_unreachable", "Orbit Link couldn't reach its server. Check this device's internet connection.")
            }
            delay(500)
        }
        ensureForwards(n, key)
    }

    /** Add the Remote Play forward preset if it isn't complete yet. Ports _link_forwards(). */
    private suspend fun ensureForwards(n: LinkNode, key: String) {
        if (!LinkNode.forwarded(n.status(), key)) {
            n.act("add-forward", JSONObject().put("peer", key).put("preset", "remote-play"))
            val deadline = System.currentTimeMillis() + 2_000
            while (!LinkNode.forwarded(n.status(), key)) {
                if (System.currentTimeMillis() >= deadline) break
                delay(100)
            }
        }
        if (!LinkNode.forwarded(n.status(), key)) {
            throw HelperError("link_ports", "A Remote Play port on this device is already in use. Close other streaming apps, then try again.")
        }
    }
}
