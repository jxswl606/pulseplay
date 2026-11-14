/*
 * PulsePlay — AGPL-3.0-only.
 * Turns the Orbit Play helper's pairing (account id + 8-digit PIN) into chiaki
 * registration keys, with no PSN. Mirrors Orbit Play service.py pair().
 */
package dev.pulseplay.stream

import com.metallic.chiaki.lib.ChiakiLog
import com.metallic.chiaki.lib.Regist
import com.metallic.chiaki.lib.RegistEventCanceled
import com.metallic.chiaki.lib.RegistEventFailed
import com.metallic.chiaki.lib.RegistEventSuccess
import com.metallic.chiaki.lib.RegistHost
import com.metallic.chiaki.lib.RegistInfo
import com.metallic.chiaki.lib.Target
import dev.pulseplay.net.HelperClient
import dev.pulseplay.net.HelperError
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.suspendCancellableCoroutine
import kotlinx.coroutines.withContext
import kotlin.coroutines.resume
import kotlin.coroutines.resumeWithException

/** A console registered for streaming: the keys chiaki needs to open a session. */
data class RegisteredConsole(
    val host: String,
    val registKey: ByteArray,
    val morning: ByteArray,
    val token: String,          // helper trust token, for the playback lease
    val ps5: Boolean = true,
)

/** Holds the registered console and persists it so next launch is one tap. */
object ConsoleStore {
    @Volatile var console: RegisteredConsole? = null
    private var prefs: android.content.SharedPreferences? = null

    private fun hex(b: ByteArray) = b.joinToString("") { "%02x".format(it) }
    private fun unhex(s: String) = ByteArray(s.length / 2) { s.substring(it * 2, it * 2 + 2).toInt(16).toByte() }

    /** Load any saved console on app start. */
    fun init(context: android.content.Context) {
        val p = context.applicationContext.getSharedPreferences("pulseplay", android.content.Context.MODE_PRIVATE)
        prefs = p
        val host = p.getString("host", null)
        val rk = p.getString("registKey", null)
        val mk = p.getString("morning", null)
        val tok = p.getString("token", null)
        if (host != null && rk != null && mk != null && tok != null) {
            runCatching { console = RegisteredConsole(host, unhex(rk), unhex(mk), tok, p.getBoolean("ps5", true)) }
        }
    }

    fun save(c: RegisteredConsole) {
        console = c
        prefs?.edit()?.apply {
            putString("host", c.host)
            putString("registKey", hex(c.registKey))
            putString("morning", hex(c.morning))
            putString("token", c.token)
            putBoolean("ps5", c.ps5)
            apply()
        }
    }

    fun forget() {
        console = null
        prefs?.edit()?.clear()?.apply()
    }
}

object Registration {
    /**
     * Full pairing. With a trust token already in hand:
     *   helper /v1/pair -> account id + 8-digit PIN -> chiaki registration -> stream keys.
     */
    suspend fun register(ip: String, token: String): RegisteredConsole = withContext(Dispatchers.IO) {
        val pair = HelperClient.pair(ip, token)
        val log = ChiakiLog(ChiakiLog.Level.ALL.value) { _, _ -> }

        var regist: Regist? = null
        val registHost: RegistHost = try {
            suspendCancellableCoroutine { cont ->
                try {
                    regist = Regist(
                        RegistInfo(Target.PS5_1, ip, false, null, pair.accountId, pair.pin),
                        log,
                    ) { event ->
                        when (event) {
                            is RegistEventSuccess -> cont.resume(event.host)
                            RegistEventFailed -> cont.resumeWithException(
                                HelperError("pair_failed", "The PS5 rejected pairing. Keep the same user logged in and try again.")
                            )
                            RegistEventCanceled -> cont.resumeWithException(
                                HelperError("expired", "Pairing was canceled. Try again.")
                            )
                        }
                    }
                } catch (e: Exception) {
                    cont.resumeWithException(e)
                }
                cont.invokeOnCancellation { regist?.stop() }
            }
        } finally {
            regist?.dispose()
        }

        RegisteredConsole(ip, registHost.rpRegistKey, registHost.rpKey, token).also { ConsoleStore.save(it) }
    }
}
