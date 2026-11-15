/*
 * PulsePlay — AGPL-3.0-only.
 * A lightweight network test (in the spirit of Steam Remote Play's): time a batch
 * of TCP connects to the PS5 helper to get latency, jitter and packet loss, then
 * recommend a quality preset for the current connection.
 *
 * Note: this measures latency/jitter/loss, which is what decides whether a stream
 * feels responsive. True throughput (Mbps) needs the console to send a test blob —
 * that arrives once our own helper exposes a /v1/speedtest endpoint.
 */
package dev.pulseplay.net

import dev.pulseplay.stream.Preset
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import java.net.InetSocketAddress
import java.net.Socket
import kotlin.math.abs

object NetworkTest {

    data class Result(
        val reachable: Boolean,
        val avgMs: Int,
        val jitterMs: Int,
        val lossPct: Int,
        val connection: String,     // "LAN" / "Wi-Fi" / "Mobile data"
        val recommend: Preset,
        val grade: String,          // "Excellent" / "Good" / "Fair" / "Poor"
        val note: String,
    )

    /** One TCP connect, in milliseconds, or -1 if it failed/timed out. */
    private fun probe(host: String, timeoutMs: Int): Long {
        return try {
            val s = Socket()
            val t0 = System.nanoTime()
            s.connect(InetSocketAddress(host, HelperClient.PORT), timeoutMs)
            val dt = (System.nanoTime() - t0) / 1_000_000
            try { s.close() } catch (_: Exception) {}
            dt
        } catch (_: Exception) { -1L }
    }

    suspend fun run(host: String): Result = withContext(Dispatchers.IO) {
        val n = 12
        val times = ArrayList<Long>(n)
        repeat(n) {
            val t = probe(host, 1500)
            if (t >= 0) times.add(t)
            try { Thread.sleep(70) } catch (_: Exception) {}
        }
        val ok = times.size
        val loss = (n - ok) * 100 / n
        val conn = NetworkMonitor.label()
        if (ok == 0) {
            return@withContext Result(
                false, 0, 0, 100, conn, Preset.PERFORMANCE, "Unreachable",
                "Couldn't reach your PS5. Make sure it's on and on the same network.",
            )
        }
        val avg = (times.sum() / ok).toInt()
        val mean = times.average()
        val jitter = times.map { abs(it - mean) }.average().toInt()
        val rec = recommend(avg, jitter, loss)
        Result(true, avg, jitter, loss, conn, rec, grade(avg, jitter, loss), noteFor(rec))
    }

    private fun recommend(avg: Int, jitter: Int, loss: Int): Preset = when {
        loss > 15 -> Preset.PERFORMANCE
        NetworkMonitor.kind.value == NetKind.CELLULAR -> Preset.PERFORMANCE
        avg <= 20 && jitter <= 8 && loss == 0 -> Preset.QUALITY
        avg <= 55 && loss <= 5 -> Preset.BALANCED
        else -> Preset.PERFORMANCE
    }

    private fun grade(avg: Int, jitter: Int, loss: Int): String = when {
        loss > 15 || avg > 120 -> "Poor"
        avg <= 20 && jitter <= 8 && loss == 0 -> "Excellent"
        avg <= 55 && loss <= 5 -> "Good"
        else -> "Fair"
    }

    private fun noteFor(p: Preset): String = when (p) {
        Preset.QUALITY -> "Fast, steady connection — you can run 1080p60."
        Preset.BALANCED -> "Solid connection — 1080p60 should hold up well."
        Preset.PERFORMANCE -> "Latency/jitter is high — 720p60 will feel the smoothest."
        Preset.CUSTOM -> ""
    }
}
