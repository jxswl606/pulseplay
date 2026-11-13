/*
 * PulsePlay — free, open-source Remote Play for jailbroken PS5s (no PSN).
 * Copyright (C) 2026 PulsePlay contributors. Licensed AGPL-3.0-only.
 * See <https://www.gnu.org/licenses/>.
 */
package dev.pulseplay

import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.darkColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.input.TextFieldValue
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.compose.foundation.text.KeyboardOptions
import dev.pulseplay.net.HelperClient
import dev.pulseplay.net.HelperError
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

private val Background = Color(0xFF0E1116)
private val SurfaceColor = Color(0xFF161B22)
private val Accent = Color(0xFF3B82F6)
private val OnDark = Color(0xFFE6EDF3)
private val Muted = Color(0xFF8B949E)
private val Good = Color(0xFF3FB950)
private val Bad = Color(0xFFF85149)

private val PulsePlayColors = darkColorScheme(
    primary = Accent, background = Background, surface = SurfaceColor,
    onBackground = OnDark, onSurface = OnDark,
)

private enum class Screen { HOME, PAIR }

class MainActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()
        setContent {
            MaterialTheme(colorScheme = PulsePlayColors) {
                Surface(modifier = Modifier.fillMaxSize(), color = Background) {
                    var screen by remember { mutableStateOf(Screen.HOME) }
                    when (screen) {
                        Screen.HOME -> HomeScreen(onAddPs5 = { screen = Screen.PAIR })
                        Screen.PAIR -> PairScreen(onBack = { screen = Screen.HOME })
                    }
                }
            }
        }
    }
}

@Composable
private fun HomeScreen(onAddPs5: () -> Unit) {
    Column(
        modifier = Modifier.fillMaxSize().padding(24.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.Center,
    ) {
        Text("PulsePlay", color = OnDark, fontSize = 34.sp, fontWeight = FontWeight.Bold)
        Text(
            "Remote Play for jailbroken PS5 — no PSN",
            color = Muted, fontSize = 14.sp,
            modifier = Modifier.padding(top = 6.dp, bottom = 40.dp),
        )
        Button(
            onClick = onAddPs5,
            colors = ButtonDefaults.buttonColors(containerColor = Accent),
            shape = RoundedCornerShape(20.dp),
            modifier = Modifier.fillMaxWidth().height(120.dp),
        ) {
            Text("Play", color = Color.White, fontSize = 28.sp, fontWeight = FontWeight.SemiBold)
        }
        TextButton(onClick = onAddPs5, modifier = Modifier.padding(top = 16.dp)) {
            Text("Add your PS5", color = Accent, fontSize = 15.sp)
        }
    }
}

@Composable
private fun PairScreen(onBack: () -> Unit) {
    val scope = rememberCoroutineScope()
    var ip by remember { mutableStateOf(TextFieldValue("192.168.")) }
    var code by remember { mutableStateOf(TextFieldValue("")) }
    var busy by remember { mutableStateOf(false) }
    var status by remember { mutableStateOf("") }
    var ok by remember { mutableStateOf(false) }

    Column(
        modifier = Modifier.fillMaxSize().padding(24.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.Center,
    ) {
        Text("Pair with your PS5", color = OnDark, fontSize = 24.sp, fontWeight = FontWeight.Bold)
        Text(
            "Run the Orbit Play helper on your PS5, then enter its local IP and the 6-digit code it shows.",
            color = Muted, fontSize = 13.sp,
            modifier = Modifier.padding(top = 8.dp, bottom = 24.dp),
        )
        OutlinedTextField(
            value = ip, onValueChange = { ip = it },
            label = { Text("PS5 IP address") },
            singleLine = true,
            keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Number),
            modifier = Modifier.fillMaxWidth(),
        )
        OutlinedTextField(
            value = code,
            onValueChange = { if (it.text.length <= 6) code = it },
            label = { Text("6-digit code") },
            singleLine = true,
            keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Number),
            modifier = Modifier.fillMaxWidth().padding(top = 12.dp),
        )
        Button(
            enabled = !busy,
            onClick = {
                status = ""
                busy = true
                scope.launch {
                    try {
                        val token = withContext(Dispatchers.IO) {
                            HelperClient.unlock(ip.text.trim(), code.text.trim())
                        }
                        ok = true
                        status = "Paired! Device trusted (token ${token.take(8)}…)."
                    } catch (e: HelperError) {
                        ok = false
                        status = e.message ?: "Pairing failed."
                    } finally {
                        busy = false
                    }
                }
            },
            colors = ButtonDefaults.buttonColors(containerColor = Accent),
            shape = RoundedCornerShape(14.dp),
            modifier = Modifier.fillMaxWidth().height(54.dp).padding(top = 20.dp),
        ) {
            Text(if (busy) "Connecting…" else "Connect", color = Color.White, fontSize = 17.sp)
        }
        if (status.isNotEmpty()) {
            Text(
                status, color = if (ok) Good else Bad, fontSize = 14.sp,
                modifier = Modifier.padding(top = 16.dp),
            )
        }
        TextButton(onClick = onBack, modifier = Modifier.padding(top = 8.dp)) {
            Text("Back", color = Muted, fontSize = 14.sp)
        }
    }
}
