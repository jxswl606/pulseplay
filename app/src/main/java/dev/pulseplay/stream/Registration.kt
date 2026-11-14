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

/** Holds the last registered console so the stream screen can reach it. */
object ConsoleStore {
    @Volatile var console: RegisteredConsole? = null
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

        RegisteredConsole(ip, registHost.rpRegistKey, registHost.rpKey, token).also { ConsoleStore.console = it }
    }
}
