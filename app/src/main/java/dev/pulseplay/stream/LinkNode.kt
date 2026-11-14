/*
 * PulsePlay — AGPL-3.0-only.
 * Runs the bundled Orbit Link node (liborbitlinknode.so) as a background process
 * and talks to its loopback JSON API. Mirrors Orbit Play app/orbit_play/link.py.
 */
package dev.pulseplay.stream

import android.content.Context
import org.json.JSONObject
import java.io.File
import java.net.HttpURLConnection
import java.net.ServerSocket
import java.net.URL

class LinkNode(context: Context) {
    private val nodePath = File(context.applicationInfo.nativeLibraryDir, "liborbitlinknode.so")
    private val dataDir = File(context.filesDir, "orbit-link").apply { mkdirs() }
    private var process: Process? = null
    @Volatile var port: Int = 0
        private set

    val available: Boolean get() = nodePath.canExecute()

    @Synchronized
    fun start() {
        if (process?.isAlive == true) return
        if (!available) throw HelperError("link_missing", "This build does not include Orbit Link.")
        val p = freePort()
        val proc = ProcessBuilder(
            nodePath.absolutePath,
            "--data", dataDir.absolutePath,
            "--listen", "127.0.0.1:$p",
            "--no-browser", "--exit-with-stdin",
        ).redirectErrorStream(true).start()
        process = proc
        port = p
        // Wait for the API to answer.
        val deadline = System.currentTimeMillis() + 10_000
        while (true) {
            try { call("GET", "status"); return } catch (_: Exception) {
                if (!proc.isAlive || System.currentTimeMillis() >= deadline) {
                    stop(); throw HelperError("link_failed", "Orbit Link could not start on this device.")
                }
                Thread.sleep(150)
            }
        }
    }

    @Synchronized
    fun stop() {
        val p = process ?: return
        process = null
        try { p.outputStream.close() } catch (_: Exception) {}   // --exit-with-stdin: closing stdin stops it
        try { if (!p.waitFor(3, java.util.concurrent.TimeUnit.SECONDS)) p.destroyForcibly() } catch (_: Exception) { p.destroyForcibly() }
    }

    fun status(): JSONObject = call("GET", "status")

    /** POST an action; returns its JSON ({ok:true,...}) or throws. */
    fun act(action: String, fields: JSONObject): JSONObject = call("POST", action, fields)

    private fun call(method: String, action: String, body: JSONObject? = null): JSONObject {
        if (port == 0) throw HelperError("link_failed", "Orbit Link is not running.")
        val path = if (method == "GET") "/api/status" else "/api/$action"
        val conn = (URL("http", "127.0.0.1", port, path).openConnection() as HttpURLConnection).apply {
            requestMethod = method
            connectTimeout = 20_000; readTimeout = 20_000; useCaches = false
            setRequestProperty("Connection", "close")
            if (body != null) {
                doOutput = true
                setRequestProperty("Content-Type", "application/json")
            }
        }
        try {
            if (body != null) conn.outputStream.use { it.write(body.toString().toByteArray()) }
            val stream = if (conn.responseCode >= 400) conn.errorStream else conn.inputStream
            val text = stream?.bufferedReader()?.use { it.readText() } ?: "{}"
            return JSONObject(text)
        } catch (e: Exception) {
            throw HelperError("link_failed", "Orbit Link on this device did not answer.")
        } finally { conn.disconnect() }
    }

    companion object {
        /** All Remote Play ports (plus the helper's) the node must forward. */
        val REMOTE_PLAY_PORTS = listOf(
            "tcp" to 9295, "udp" to 9295, "udp" to 9296, "udp" to 9297, "udp" to 9302, "tcp" to 34179,
        )

        private fun freePort(): Int = ServerSocket(0).use { it.localPort }

        /** Is every Remote Play port forwarded for this peer? */
        fun forwarded(status: JSONObject, key: String): Boolean {
            val wanted = REMOTE_PLAY_PORTS.toMutableSet()
            val arr = status.optJSONArray("forwards") ?: return false
            for (i in 0 until arr.length()) {
                val f = arr.optJSONObject(i) ?: continue
                if (f.optString("peer") != key) continue
                wanted.remove(f.optString("protocol") to f.optInt("port"))
            }
            return wanted.isEmpty()
        }

        /** Whether a peer with exactly this key is present in the node's status. */
        fun hasPeer(status: JSONObject, key: String): Boolean {
            val arr = status.optJSONArray("peers") ?: return false
            for (i in 0 until arr.length()) {
                if (arr.optJSONObject(i)?.optString("key") == key) return true
            }
            return false
        }

        /** Whether the peer with this key is present and reporting itself online. */
        fun peerOnline(status: JSONObject, key: String): Boolean {
            val arr = status.optJSONArray("peers") ?: return false
            for (i in 0 until arr.length()) {
                val p = arr.optJSONObject(i) ?: continue
                if (p.optString("key") == key && p.optBoolean("online")) return true
            }
            return false
        }

        /** The online PS5 peer object for this key, if it is reachable now. */
        fun peerReady(status: JSONObject, key: String): Boolean {
            val arr = status.optJSONArray("peers") ?: return false
            for (i in 0 until arr.length()) {
                val p = arr.optJSONObject(i) ?: continue
                if (p.optString("key") == key && p.optBoolean("online") &&
                    p.optString("path") in setOf("direct", "relay")
                ) return true
            }
            return false
        }

        /** The PS5 peer's key in this node's status, if present. */
        fun ps5Key(status: JSONObject): String? {
            val arr = status.optJSONArray("peers") ?: return null
            for (i in 0 until arr.length()) {
                val p = arr.optJSONObject(i) ?: continue
                if (p.optString("kind") == "ps5") {
                    val k = p.optString("key")
                    if (k.isNotEmpty()) return k
                }
            }
            return null
        }
    }
}
