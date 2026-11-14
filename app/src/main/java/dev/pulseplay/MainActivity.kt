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
import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.core.RepeatMode
import androidx.compose.animation.core.animateFloat
import androidx.compose.animation.core.infiniteRepeatable
import androidx.compose.animation.core.rememberInfiniteTransition
import androidx.compose.animation.core.tween
import androidx.compose.animation.fadeIn
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.clickable
import androidx.compose.foundation.lazy.LazyRow
import androidx.compose.foundation.lazy.grid.GridCells
import androidx.compose.foundation.lazy.grid.LazyVerticalGrid
import androidx.compose.foundation.lazy.grid.items as gridItems
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
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
import androidx.compose.ui.draw.clip
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.input.TextFieldValue
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.compose.foundation.text.KeyboardOptions
import dev.pulseplay.net.HelperClient
import dev.pulseplay.net.HelperError
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

// ---- Console-native palette ----
private val Bg0 = Color(0xFF070A12)
private val Bg1 = Color(0xFF0A101F)
private val Bg2 = Color(0xFF122038)
private val Panel = Color(0xFF131C2E)
private val Accent = Color(0xFF1B8CFF)
private val OnDark = Color(0xFFE8EEF8)
private val Muted = Color(0xFF7B8AA3)
private val Good = Color(0xFF3AD17F)

private val Colors = darkColorScheme(
    primary = Accent, background = Bg0, surface = Panel,
    onBackground = OnDark, onSurface = OnDark,
)

private fun screenBg() = Brush.verticalGradient(listOf(Bg2, Bg1, Bg0))

private enum class Screen { SPLASH, HOME, PAIR }

class MainActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()
        setContent {
            MaterialTheme(colorScheme = Colors) {
                var screen by remember { mutableStateOf(Screen.SPLASH) }
                Box(Modifier.fillMaxSize().background(screenBg())) {
                    when (screen) {
                        Screen.SPLASH -> SplashScreen(onDone = { screen = Screen.HOME })
                        Screen.HOME -> HomeScreen(onAddPs5 = { screen = Screen.PAIR })
                        Screen.PAIR -> PairScreen(onBack = { screen = Screen.HOME })
                    }
                }
            }
        }
    }
}

/** The Signal logo mark: a play triangle with two broadcast arcs. */
@Composable
private fun LogoMark(modifier: Modifier = Modifier, color: Color = Color.White) {
    Canvas(modifier) {
        val w = size.width
        val h = size.height
        val tri = Path().apply {
            moveTo(w * 0.30f, h * 0.28f)
            lineTo(w * 0.60f, h * 0.50f)
            lineTo(w * 0.30f, h * 0.72f)
            close()
        }
        drawPath(tri, color)
        val stroke = Stroke(width = w * 0.055f)
        // two arcs opening to the right
        drawArc(color, -55f, 110f, false,
            topLeft = Offset(w * 0.40f, h * 0.22f),
            size = androidx.compose.ui.geometry.Size(w * 0.44f, h * 0.56f), style = stroke)
        drawArc(color.copy(alpha = 0.5f), -58f, 116f, false,
            topLeft = Offset(w * 0.46f, h * 0.10f),
            size = androidx.compose.ui.geometry.Size(w * 0.60f, h * 0.80f), style = stroke)
    }
}

@Composable
private fun SplashScreen(onDone: () -> Unit) {
    val t = rememberInfiniteTransition(label = "glow")
    val glow by t.animateFloat(
        initialValue = 0.45f, targetValue = 1f,
        animationSpec = infiniteRepeatable(tween(1050), RepeatMode.Reverse), label = "g",
    )
    androidx.compose.runtime.LaunchedEffect(Unit) {
        delay(3000); onDone()
    }
    Column(
        Modifier.fillMaxSize(), horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.Center,
    ) {
        Box(contentAlignment = Alignment.Center) {
            // glow
            Box(
                Modifier.size(260.dp).clip(RoundedCornerShape(130.dp))
                    .background(Brush.radialGradient(listOf(Accent.copy(alpha = 0.55f * glow), Color.Transparent)))
            )
            Box(
                Modifier.size(150.dp).clip(RoundedCornerShape(38.dp))
                    .background(Brush.linearGradient(listOf(Bg2, Bg0)))
                    .padding(34.dp),
                contentAlignment = Alignment.Center,
            ) { LogoMark(Modifier.fillMaxSize(), Accent.copy(alpha = 0.6f + 0.4f * glow)) }
        }
        Spacer(Modifier.height(28.dp))
        Row {
            Text("Pulse", color = OnDark, fontSize = 30.sp, fontWeight = FontWeight.Bold)
            Text("Play", color = Accent, fontSize = 30.sp, fontWeight = FontWeight.Bold)
        }
    }
}

@Composable
private fun HomeScreen(onAddPs5: () -> Unit) {
    Row(Modifier.fillMaxSize()) {
        // ---- side menu ----
        Column(
            Modifier.width(210.dp).fillMaxHeight()
                .background(Color(0xFF0A0F1A).copy(alpha = 0.6f)).padding(16.dp)
        ) {
            Row(verticalAlignment = Alignment.CenterVertically, modifier = Modifier.padding(start = 6.dp, bottom = 22.dp)) {
                Box(
                    Modifier.size(30.dp).clip(RoundedCornerShape(9.dp))
                        .background(Brush.linearGradient(listOf(Accent, Color(0xFF0B4EA0)))).padding(6.dp)
                ) { LogoMark(Modifier.fillMaxSize(), Color.White) }
                Spacer(Modifier.width(10.dp))
                Row {
                    Text("Pulse", color = Color.White, fontSize = 17.sp, fontWeight = FontWeight.SemiBold)
                    Text("Play", color = Accent, fontSize = 17.sp, fontWeight = FontWeight.SemiBold)
                }
            }
            NavItem("Home", selected = true)
            NavItem("Library", selected = false)
            NavItem("Settings", selected = false)
            Spacer(Modifier.weight(1f))
            Row(
                Modifier.fillMaxWidth().clip(RoundedCornerShape(12.dp))
                    .background(Color.White.copy(alpha = 0.03f)).padding(12.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Box(Modifier.size(9.dp).clip(RoundedCornerShape(5.dp)).background(Good))
                Spacer(Modifier.width(10.dp))
                Column {
                    Text("PlayStation 5", color = OnDark, fontSize = 13.sp, fontWeight = FontWeight.SemiBold)
                    Text("Ready · LAN", color = Muted, fontSize = 11.sp)
                }
            }
        }
        // ---- content ----
        Column(Modifier.weight(1f).fillMaxHeight().padding(horizontal = 28.dp, vertical = 26.dp)) {
            Text("Home", color = Color.White, fontSize = 24.sp, fontWeight = FontWeight.SemiBold)
            Spacer(Modifier.height(18.dp))
            // console hero
            Row(
                Modifier.fillMaxWidth().clip(RoundedCornerShape(18.dp))
                    .background(Brush.horizontalGradient(listOf(Accent.copy(alpha = 0.16f), Panel.copy(alpha = 0.4f))))
                    .padding(18.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Box(
                    Modifier.size(58.dp).clip(RoundedCornerShape(15.dp)).background(Bg0).padding(12.dp),
                    contentAlignment = Alignment.Center,
                ) { LogoMark(Modifier.fillMaxSize(), Accent) }
                Spacer(Modifier.width(14.dp))
                Column(Modifier.weight(1f)) {
                    Text("PlayStation 5", color = OnDark, fontSize = 17.sp, fontWeight = FontWeight.SemiBold)
                    Text("10.0.0.186 · Ready to stream", color = Muted, fontSize = 12.sp)
                }
                Button(
                    onClick = onAddPs5,
                    colors = ButtonDefaults.buttonColors(containerColor = Accent),
                    shape = RoundedCornerShape(14.dp),
                    modifier = Modifier.height(54.dp).width(150.dp),
                ) { Text("Play", color = Color.White, fontSize = 18.sp, fontWeight = FontWeight.SemiBold) }
            }
            Spacer(Modifier.height(24.dp))
            Text("Continue playing", color = Color(0xFFCDD8EA), fontSize = 14.sp, fontWeight = FontWeight.SemiBold)
            Spacer(Modifier.height(12.dp))
            val covers = listOf(
                Color(0xFF6A1020), Color(0xFFB81E2E), Color(0xFF1F4F7D), Color(0xFF5A3A1F),
            )
            LazyRow(horizontalArrangement = Arrangement.spacedBy(14.dp)) {
                items(covers) { c ->
                    Box(
                        Modifier.width(120.dp).height(156.dp).clip(RoundedCornerShape(13.dp))
                            .background(Brush.verticalGradient(listOf(c, Bg0)))
                    )
                }
            }
            Spacer(Modifier.height(22.dp))
            Text("Your library", color = Color(0xFFCDD8EA), fontSize = 14.sp, fontWeight = FontWeight.SemiBold)
            Spacer(Modifier.height(12.dp))
            val lib = listOf(
                Color(0xFF243A5E), Color(0xFF4A2030), Color(0xFF1F4F5A), Color(0xFF3A2F5E),
                Color(0xFF5A3A1F), Color(0xFF20405E), Color(0xFF4A1F3A), Color(0xFF2F4A3A),
            )
            LazyVerticalGrid(
                columns = GridCells.Adaptive(96.dp),
                horizontalArrangement = Arrangement.spacedBy(11.dp),
                verticalArrangement = Arrangement.spacedBy(11.dp),
                modifier = Modifier.weight(1f),
            ) {
                gridItems(lib) { c ->
                    Box(
                        Modifier.aspectRatio(0.77f).clip(RoundedCornerShape(11.dp))
                            .background(Brush.verticalGradient(listOf(c, Bg0)))
                    )
                }
            }
        }
    }
}

@Composable
private fun NavItem(label: String, selected: Boolean) {
    Row(
        Modifier.fillMaxWidth().padding(vertical = 2.dp).clip(RoundedCornerShape(11.dp))
            .background(if (selected) Accent.copy(alpha = 0.14f) else Color.Transparent)
            .padding(horizontal = 14.dp, vertical = 12.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Text(label, color = if (selected) Color.White else Muted, fontSize = 14.sp, fontWeight = FontWeight.Medium)
    }
}

@Composable
private fun PairScreen(onBack: () -> Unit) {
    val scope = rememberCoroutineScope()
    var ip by remember { mutableStateOf(TextFieldValue("10.0.0.")) }
    var code by remember { mutableStateOf(TextFieldValue("")) }
    var busy by remember { mutableStateOf(false) }
    var status by remember { mutableStateOf("") }
    var ok by remember { mutableStateOf(false) }

    Column(
        Modifier.fillMaxSize().padding(28.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.Center,
    ) {
        Text("Pair with your PS5", color = Color.White, fontSize = 24.sp, fontWeight = FontWeight.Bold)
        Text(
            "Run the Orbit Play helper on your PS5, then enter its local IP and the 6-digit code it shows.",
            color = Muted, fontSize = 13.sp, modifier = Modifier.padding(top = 8.dp, bottom = 22.dp),
        )
        OutlinedTextField(
            value = ip, onValueChange = { ip = it }, label = { Text("PS5 IP address") },
            singleLine = true, keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Number),
            modifier = Modifier.fillMaxWidth(),
        )
        OutlinedTextField(
            value = code, onValueChange = { if (it.text.length <= 6) code = it }, label = { Text("6-digit code") },
            singleLine = true, keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Number),
            modifier = Modifier.fillMaxWidth().padding(top = 12.dp),
        )
        Button(
            enabled = !busy,
            onClick = {
                status = ""; busy = true
                scope.launch {
                    try {
                        val token = withContext(Dispatchers.IO) { HelperClient.unlock(ip.text.trim(), code.text.trim()) }
                        ok = true; status = "Paired! Device trusted (token ${token.take(8)}…)."
                    } catch (e: HelperError) {
                        ok = false; status = e.message ?: "Pairing failed."
                    } finally { busy = false }
                }
            },
            colors = ButtonDefaults.buttonColors(containerColor = Accent),
            shape = RoundedCornerShape(14.dp),
            modifier = Modifier.fillMaxWidth().height(54.dp).padding(top = 20.dp),
        ) { Text(if (busy) "Connecting…" else "Connect", color = Color.White, fontSize = 17.sp) }
        if (status.isNotEmpty()) {
            Text(status, color = if (ok) Good else Color(0xFFF85149), fontSize = 14.sp, modifier = Modifier.padding(top = 16.dp))
        }
        TextButton(onClick = onBack, modifier = Modifier.padding(top = 8.dp)) {
            Text("Back", color = Muted, fontSize = 14.sp)
        }
    }
}
