/*
 * PulsePlay — Remote Play for jailbroken PS5s (no PSN).
 * Copyright (C) 2026 PulsePlay contributors. Licensed AGPL-3.0-only.
 *
 * Client for the PS5-side Orbit Play helper's control protocol.
 * Protocol verified from Orbit Play v0.8.0 (app/orbit_play/helper.py):
 *   - plain HTTP to TCP 34179, numeric private-LAN IPv4 only
 *   - headers: X-Orbit-Play-Client: 1, Authorization: Bearer <64-hex token>,
 *     X-Orbit-Play-Code: <6 digits>
 *   - POST /v1/unlock + 6-digit code  ->  { "token": "<64 hex>" }
 */
package dev.pulseplay.net

import org.json.JSONObject
import java.net.HttpURLConnection
import java.net.Inet4Address
import java.net.InetAddress
import java.net.URL

/** A failure with a short, user-facing message and a protocol code (for logic/tests). */
class HelperError(val code: String, message: String) : Exception(message)

object HelperClient {
    const val PORT = 34179
    private const val TIMEOUT_MS = 6000

    /** True only for RFC-1918 private IPv4 — the helper refuses anything else. */
    fun isPrivateLanIpv4(value: String): Boolean {
        val addr = try {
            InetAddress.getByName(value.trim())
        } catch (_: Exception) {
            return false
        }
        if (addr !is Inet4Address) return false
        // Reject if the text wasn't a literal IP (getByName can resolve names).
        if (addr.hostAddress != value.trim()) return false
        val b = addr.address
        val o0 = b[0].toInt() and 0xFF
        val o1 = b[1].toInt() and 0xFF
        return when (o0) {
            10 -> true                       // 10.0.0.0/8
            172 -> o1 in 16..31              // 172.16.0.0/12
            192 -> o1 == 168                 // 192.168.0.0/16
            else -> false
        }
    }

    /**
     * Trust this device with the helper using the 6-digit code shown on the PS5.
     * Returns the 64-hex device token on success; throws HelperError otherwise.
     */
    fun unlock(host: String, code: String): String {
        if (!isPrivateLanIpv4(host)) {
            throw HelperError("address", "Enter your PS5's local IP (like 192.168.x.x).")
        }
        if (code.length != 6 || !code.all { it.isDigit() }) {
            throw HelperError("code", "Enter the six-digit code from your PS5 notification.")
        }
        val data = request("POST", host, "/v1/unlock", code = code, token = null)
        val token = data.optString("token", "")
        if (token.length != 64 || !token.all { it in "0123456789abcdef" }) {
            throw HelperError("protocol", "The helper returned an invalid device token.")
        }
        return token
    }

    /** Result of the helper's native pairing: the console's account id and the 8-digit PIN. */
    class PairInfo(val accountId: ByteArray, val pin: Int)

    private fun isProof(p: String) = p.length == 64 && p.all { it in "0123456789abcdef" }

    /** GET /v1/session: a one-time review proof for pairing or for a playback lease. */
    fun sessionProof(host: String, token: String): String {
        val deadline = System.currentTimeMillis() + 4000
        while (true) {
            try {
                val proof = request("GET", host, "/v1/session", token = token).optString("proof", "")
                if (!isProof(proof)) throw HelperError("protocol", "The helper returned an invalid connection review.")
                return proof
            } catch (e: HelperError) {
                if (e.code != "busy" || System.currentTimeMillis() >= deadline) throw e
                Thread.sleep(200)
            }
        }
    }

    /** POST /v1/pair then poll GET /v1/pair until the PS5 has generated the registration PIN. */
    fun pair(host: String, token: String): PairInfo {
        request("POST", host, "/v1/pair", token = token, proof = sessionProof(host, token))
        val deadline = System.currentTimeMillis() + 20000
        while (System.currentTimeMillis() < deadline) {
            val p = request("GET", host, "/v1/pair", token = token)
            when (p.optString("state")) {
                "ready" -> {
                    val pin = p.optString("pin", "")
                    val id = try {
                        android.util.Base64.decode(p.optString("account_id", ""), android.util.Base64.DEFAULT)
                    } catch (_: Exception) { ByteArray(0) }
                    if (pin.length != 8 || !pin.all { it.isDigit() } || id.size != 8) {
                        throw HelperError("protocol", "The helper returned an invalid pairing response.")
                    }
                    return PairInfo(id, pin.toInt())
                }
                "preparing" -> Thread.sleep(250)
                else -> throw HelperError("expired", "Pairing expired. Try pairing again.")
            }
        }
        throw HelperError("native_timeout", "The PS5 pairing API did not respond. Restart the helper and retry.")
    }

    /** Stop a pairing attempt that did not finish. */
    fun cancel(host: String, token: String) {
        try { request("POST", host, "/v1/cancel", token = token) } catch (_: HelperError) {}
    }

    /** POST /v1/session: open (or renew, every 10 s) the 45-second playback lease. */
    fun sessionOpen(host: String, token: String, proof: String) {
        val r = request("POST", host, "/v1/session", token = token, proof = proof)
        if (r.optString("state") != "active") {
            throw HelperError("session_expired", "The PS5 connection ended. Pair again.")
        }
    }

    /** POST /v1/session/end: release the playback lease after the stream stops. */
    fun sessionEnd(host: String, token: String, proof: String) {
        try { request("POST", host, "/v1/session/end", token = token, proof = proof) } catch (_: HelperError) {}
    }

    /** Low-level request mirroring helper.py; returns the parsed JSON object. */
    private fun request(
        method: String,
        host: String,
        path: String,
        code: String? = null,
        token: String? = null,
        proof: String? = null,
    ): JSONObject {
        val url = URL("http", host, PORT, path)
        val conn = (url.openConnection() as HttpURLConnection).apply {
            requestMethod = method
            connectTimeout = TIMEOUT_MS
            readTimeout = TIMEOUT_MS
            useCaches = false
            setRequestProperty("X-Orbit-Play-Client", "1")
            setRequestProperty("Connection", "close")
            if (token != null) setRequestProperty("Authorization", "Bearer $token")
            if (code != null) setRequestProperty("X-Orbit-Play-Code", code)
            if (proof != null) setRequestProperty("X-Orbit-Play-Review", proof)
        }
        try {
            val status = conn.responseCode
            val stream = if (status >= 400) conn.errorStream else conn.inputStream
            val body = stream?.bufferedReader()?.use { it.readText() } ?: ""
            val json = try {
                JSONObject(body)
            } catch (_: Exception) {
                throw HelperError("protocol", "This address did not return a valid helper response.")
            }
            if (status >= 400) {
                val c = json.optString("code", "helper")
                throw HelperError(c, messageFor(c))
            }
            return json
        } catch (e: HelperError) {
            throw e
        } catch (_: Exception) {
            throw HelperError(
                "unreachable",
                "The helper isn't responding. Keep the PS5 on the same network with the Orbit Play helper running.",
            )
        } finally {
            conn.disconnect()
        }
    }

    /** Subset of the helper's error vocabulary that matters for pairing. */
    private fun messageFor(code: String): String = when (code) {
        "locked" -> "Enter the code from the PS5 notification to trust this device."
        "code" -> "That code did not match. Check the PS5 notification."
        "rate_limit" -> "Too many code attempts. Wait a minute before trying again."
        "inactive" -> "Log into the user you want to play as on your PS5."
        "busy" -> "The PS5 is already preparing a pairing request. Wait for it to finish."
        "streaming" -> "Someone is already streaming from this PS5. Stop that first."
        "activation_required" -> "This PS5 profile has no local account ID yet. Set it up once with the Orbit Play desktop app."
        "remote_play_disabled" -> "Enable Remote Play for this PS5 profile, then pair again."
        "user_changed" -> "The active PS5 user changed. Pair again."
        "native_error" -> "The PS5 Remote Play API returned an error. Restart the helper and retry."
        "live_failed" -> "The helper could not prepare the live connection."
        "session_expired" -> "The PS5 connection ended. Pair again."
        else -> "The PS5 helper could not complete that request."
    }
}
