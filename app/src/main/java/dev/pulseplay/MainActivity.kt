/*
 * PulsePlay — free, open-source Remote Play for jailbroken PS5s (no PSN).
 * Copyright (C) 2026 Jxswl606. Licensed AGPL-3.0-only.
 */
package dev.pulseplay

import android.os.Bundle
import android.view.KeyEvent
import android.view.MotionEvent
import android.view.SurfaceHolder
import android.view.SurfaceView
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.compose.animation.core.RepeatMode
import androidx.compose.animation.core.animateFloat
import androidx.compose.animation.core.infiniteRepeatable
import androidx.compose.animation.core.rememberInfiniteTransition
import androidx.compose.animation.core.tween
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
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
import androidx.compose.foundation.layout.safeDrawingPadding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyRow
import androidx.compose.foundation.lazy.grid.GridCells
import androidx.compose.foundation.lazy.grid.LazyVerticalGrid
import androidx.compose.foundation.lazy.grid.items as gridItems
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.ui.draw.alpha
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.darkColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.geometry.Offset
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.interaction.collectIsPressedAsState
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.input.TextFieldValue
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.compose.ui.viewinterop.AndroidView
import dev.pulseplay.net.HelperClient
import dev.pulseplay.net.HelperError
import dev.pulseplay.net.NetworkTest
import dev.pulseplay.stream.ActiveStream
import dev.pulseplay.stream.AwayManager
import dev.pulseplay.stream.ConsoleStore
import dev.pulseplay.stream.Discovery
import dev.pulseplay.stream.GamepadOverlay
import dev.pulseplay.stream.Registration
import dev.pulseplay.stream.StreamPrefs
import dev.pulseplay.stream.Res
import dev.pulseplay.stream.Fps
import dev.pulseplay.stream.Preset
import dev.pulseplay.stream.FillMode
import dev.pulseplay.stream.ConsoleStatus
import dev.pulseplay.stream.StreamSession
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
private val Rest = Color(0xFFE0B24A)          // rest-mode amber
private val Bad  = Color(0xFFF85149)          // offline red

/** Status dot color for the console's live power state (Remote Play style). */
private fun consoleDotColor(s: ConsoleStatus.State) = when (s) {
    ConsoleStatus.State.ONLINE -> Good
    ConsoleStatus.State.REST -> Rest
    ConsoleStatus.State.OFFLINE -> Bad
    ConsoleStatus.State.CHECKING -> Muted
}

private val Colors = darkColorScheme(
    primary = Accent, background = Bg0, surface = Panel,
    onBackground = OnDark, onSurface = OnDark,
)

private val Ink = Color(0xFF05070C)          // OLED base
private val Card = Color(0xFF0E131D)          // raised card
private val Card2 = Color(0xFF131A27)
private val Line = Color(0x14FFFFFF)
private fun screenBg() = Brush.verticalGradient(listOf(Color(0xFF0A0E16), Ink))

private enum class Screen { SPLASH, HOME, PAIR, STREAM }
private enum class Tab(val label: String) {
    HOME("Home"), CONSOLES("Consoles"), LIBRARY("Library"), CONTROLLER("Controller"), SETTINGS("Settings"), ABOUT("About")
}

class MainActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        ConsoleStore.init(this)
        StreamPrefs.init(this)
        dev.pulseplay.net.NetworkMonitor.init(this)
        dev.pulseplay.stream.ConsoleStatus.start()
        enableEdgeToEdge()
        hideSystemBars()
        setContent {
            MaterialTheme(colorScheme = Colors) {
                var screen by remember { mutableStateOf(Screen.SPLASH) }
                var streamKey by remember { mutableStateOf(0) }   // bump to reconnect the stream
                Box(Modifier.fillMaxSize().background(screenBg())) {
                    when (screen) {
                        Screen.SPLASH -> SplashScreen(onDone = { screen = Screen.HOME })
                        Screen.HOME -> HomeScreen(
                            onStream = { gid ->
                                ActiveStream.pendingGameId = gid
                                screen = if (ConsoleStore.console != null) Screen.STREAM else Screen.PAIR
                            },
                            onPair = { screen = Screen.PAIR },   // reach pairing even when a console exists
                        )
                        Screen.PAIR -> PairScreen(
                            onBack = { screen = Screen.HOME },
                            onPaired = { screen = Screen.STREAM },   // registered → stream right away
                        )
                        // key(streamKey): bumping it fully recreates the stream (new session +
                        // surface, correctly bound) — a clean reconnect when quality changes.
                        Screen.STREAM -> androidx.compose.runtime.key(streamKey) {
                            StreamScreen(onExit = { screen = Screen.HOME }, onReconnect = { streamKey++ })
                        }
                    }
                }
            }
        }
    }

    // Route a physical controller's input into the live stream.
    override fun dispatchKeyEvent(event: KeyEvent): Boolean {
        if (ActiveStream.session != null && ActiveStream.onKey(event)) return true
        return super.dispatchKeyEvent(event)
    }

    override fun dispatchGenericMotionEvent(event: MotionEvent): Boolean {
        if (ActiveStream.session != null && ActiveStream.onMotion(event)) return true
        return super.dispatchGenericMotionEvent(event)
    }

    /** Hide the Android status/navigation bars for the whole app (immersive). */
    private fun hideSystemBars() {
        androidx.core.view.WindowCompat.setDecorFitsSystemWindows(window, false)
        androidx.core.view.WindowInsetsControllerCompat(window, window.decorView).apply {
            hide(androidx.core.view.WindowInsetsCompat.Type.systemBars())
            systemBarsBehavior = androidx.core.view.WindowInsetsControllerCompat.BEHAVIOR_SHOW_TRANSIENT_BARS_BY_SWIPE
        }
    }

    override fun onWindowFocusChanged(hasFocus: Boolean) {
        super.onWindowFocusChanged(hasFocus)
        if (hasFocus) hideSystemBars()   // bars can re-appear after dialogs/keyboard; keep them hidden
    }
}

/** The Signal logo mark: a play triangle with two broadcast arcs, fit inside its bounds. */
@Composable
private fun LogoMark(modifier: Modifier = Modifier, color: Color = Color.White) {
    Canvas(modifier) {
        val w = size.width
        val h = size.height
        val tri = Path().apply {
            moveTo(w * 0.20f, h * 0.28f); lineTo(w * 0.48f, h * 0.50f); lineTo(w * 0.20f, h * 0.72f); close()
        }
        drawPath(tri, color)
        val stroke = Stroke(width = w * 0.06f, cap = androidx.compose.ui.graphics.StrokeCap.Round)
        drawArc(color, -58f, 116f, false, Offset(w * 0.40f, h * 0.26f),
            androidx.compose.ui.geometry.Size(w * 0.30f, h * 0.48f), style = stroke)
        drawArc(color.copy(alpha = 0.5f), -60f, 120f, false, Offset(w * 0.50f, h * 0.14f),
            androidx.compose.ui.geometry.Size(w * 0.44f, h * 0.72f), style = stroke)
    }
}

@Composable
private fun SplashScreen(onDone: () -> Unit) {
    val t = rememberInfiniteTransition(label = "glow")
    val glow by t.animateFloat(0.35f, 1f, infiniteRepeatable(tween(1150), RepeatMode.Reverse), label = "g")
    LaunchedEffect(Unit) { delay(3000); onDone() }
    Box(Modifier.fillMaxSize().background(Color.Black), contentAlignment = Alignment.Center) {
        Column(horizontalAlignment = Alignment.CenterHorizontally) {
            Box(contentAlignment = Alignment.Center) {
                // soft radial glow
                Box(Modifier.size(280.dp).background(
                    Brush.radialGradient(listOf(Accent.copy(alpha = 0.40f * glow), Color.Transparent)), CircleShape))
                // faint bloom of the mark
                LogoMark(Modifier.size(156.dp), Accent.copy(alpha = 0.22f * glow))
                // crisp glowing sign
                LogoMark(Modifier.size(120.dp), Color(0xFF6BC0FF).copy(alpha = 0.78f + 0.22f * glow))
            }
            Spacer(Modifier.height(34.dp))
            Row {
                Text("Pulse", color = Color.White, fontSize = 30.sp, fontWeight = FontWeight.Bold,
                    style = androidx.compose.ui.text.TextStyle(
                        shadow = androidx.compose.ui.graphics.Shadow(Accent.copy(alpha = glow), blurRadius = 26f)))
                Text("Play", color = Color(0xFF6BC0FF), fontSize = 30.sp, fontWeight = FontWeight.Bold,
                    style = androidx.compose.ui.text.TextStyle(
                        shadow = androidx.compose.ui.graphics.Shadow(Accent.copy(alpha = glow), blurRadius = 30f)))
            }
        }
    }
}

@Composable
private fun HomeScreen(onStream: (String?) -> Unit, onPair: () -> Unit) {
    var tab by remember { mutableStateOf(Tab.HOME) }
    Row(Modifier.fillMaxSize()) {
        Sidebar(tab) { tab = it }
        Box(Modifier.weight(1f).fillMaxHeight().padding(horizontal = 30.dp, vertical = 26.dp)) {
            when (tab) {
                Tab.HOME -> HomeContent(onStream, navTo = { tab = it })
                Tab.CONSOLES -> ConsolesContent(onStream, onPair)
                Tab.LIBRARY -> LibraryContent(onStream)
                Tab.CONTROLLER -> ControllerContent()
                Tab.SETTINGS -> SettingsContent(onForget = { tab = Tab.HOME })
                Tab.ABOUT -> AboutContent()
            }
        }
    }
}

@Composable
private fun Sidebar(tab: Tab, onSelect: (Tab) -> Unit) {
    Column(Modifier.width(212.dp).fillMaxHeight().background(Ink).padding(16.dp)) {
        Row(verticalAlignment = Alignment.CenterVertically, modifier = Modifier.padding(start = 6.dp, top = 6.dp, bottom = 24.dp)) {
            Box(Modifier.size(30.dp).clip(RoundedCornerShape(9.dp)).background(Brush.linearGradient(listOf(Accent, Color(0xFF0B4EA0)))).padding(6.dp)) {
                LogoMark(Modifier.fillMaxSize(), Color.White)
            }
            Spacer(Modifier.width(10.dp))
            Row {
                Text("Pulse", color = Color.White, fontSize = 17.sp, fontWeight = FontWeight.SemiBold)
                Text("Play", color = Accent, fontSize = 17.sp, fontWeight = FontWeight.SemiBold)
            }
        }
        Tab.values().forEach { t -> NavItem(t, t == tab) { onSelect(t) } }
        Spacer(Modifier.weight(1f))
        Text("v1.0.0", color = Color(0xFF3C4759), fontSize = 11.sp, modifier = Modifier.padding(start = 6.dp))
    }
}

@Composable
private fun NavItem(tab: Tab, selected: Boolean, onClick: () -> Unit) {
    val tint = if (selected) Color.White else Muted
    Row(Modifier.fillMaxWidth().padding(vertical = 3.dp).clip(RoundedCornerShape(12.dp))
        .background(if (selected) Accent.copy(alpha = 0.16f) else Color.Transparent)
        .clickable(onClick = onClick).padding(horizontal = 13.dp, vertical = 12.dp),
        verticalAlignment = Alignment.CenterVertically) {
        NavIcon(tab, tint)
        Spacer(Modifier.width(13.dp))
        Text(tab.label, color = tint, fontSize = 14.sp, fontWeight = FontWeight.Medium)
    }
}

/** Minimal line icons drawn on a Canvas so we need no icon dependency. */
@Composable
private fun NavIcon(tab: Tab, tint: Color) {
    androidx.compose.foundation.Canvas(Modifier.size(19.dp)) {
        val s = size.minDimension
        val st = s * 0.11f
        val stroke = androidx.compose.ui.graphics.drawscope.Stroke(width = st, cap = androidx.compose.ui.graphics.StrokeCap.Round, join = androidx.compose.ui.graphics.StrokeJoin.Round)
        when (tab) {
            Tab.HOME -> {
                val p = androidx.compose.ui.graphics.Path().apply {
                    moveTo(s * 0.5f, s * 0.12f); lineTo(s * 0.9f, s * 0.46f)
                    lineTo(s * 0.9f, s * 0.9f); lineTo(s * 0.1f, s * 0.9f)
                    lineTo(s * 0.1f, s * 0.46f); close()
                }
                drawPath(p, tint, style = stroke)
            }
            Tab.CONSOLES -> {
                drawRoundRect(tint, topLeft = androidx.compose.ui.geometry.Offset(s * 0.12f, s * 0.2f),
                    size = androidx.compose.ui.geometry.Size(s * 0.76f, s * 0.5f),
                    cornerRadius = androidx.compose.ui.geometry.CornerRadius(s * 0.1f), style = stroke)
                drawLine(tint, androidx.compose.ui.geometry.Offset(s * 0.35f, s * 0.86f), androidx.compose.ui.geometry.Offset(s * 0.65f, s * 0.86f), strokeWidth = st, cap = androidx.compose.ui.graphics.StrokeCap.Round)
            }
            Tab.LIBRARY -> {
                for (gx in 0..1) for (gy in 0..1) drawRoundRect(tint,
                    topLeft = androidx.compose.ui.geometry.Offset(s * (0.14f + gx * 0.44f), s * (0.14f + gy * 0.44f)),
                    size = androidx.compose.ui.geometry.Size(s * 0.3f, s * 0.3f),
                    cornerRadius = androidx.compose.ui.geometry.CornerRadius(s * 0.06f), style = stroke)
            }
            Tab.CONTROLLER -> {
                drawRoundRect(tint, topLeft = androidx.compose.ui.geometry.Offset(s * 0.08f, s * 0.3f),
                    size = androidx.compose.ui.geometry.Size(s * 0.84f, s * 0.4f),
                    cornerRadius = androidx.compose.ui.geometry.CornerRadius(s * 0.2f), style = stroke)
                drawCircle(tint, s * 0.06f, androidx.compose.ui.geometry.Offset(s * 0.3f, s * 0.5f))
                drawCircle(tint, s * 0.06f, androidx.compose.ui.geometry.Offset(s * 0.7f, s * 0.5f))
            }
            Tab.SETTINGS -> {
                for (i in 0..2) {
                    val y = s * (0.26f + i * 0.24f)
                    drawLine(tint, androidx.compose.ui.geometry.Offset(s * 0.14f, y), androidx.compose.ui.geometry.Offset(s * 0.86f, y), strokeWidth = st, cap = androidx.compose.ui.graphics.StrokeCap.Round)
                    drawCircle(tint, s * 0.07f, androidx.compose.ui.geometry.Offset(s * (if (i == 1) 0.7f else 0.35f), y))
                }
            }
            Tab.ABOUT -> {
                drawCircle(tint, s * 0.4f, androidx.compose.ui.geometry.Offset(s * 0.5f, s * 0.5f), style = stroke)
                drawCircle(tint, s * 0.05f, androidx.compose.ui.geometry.Offset(s * 0.5f, s * 0.3f))
                drawLine(tint, androidx.compose.ui.geometry.Offset(s * 0.5f, s * 0.46f), androidx.compose.ui.geometry.Offset(s * 0.5f, s * 0.72f), strokeWidth = st, cap = androidx.compose.ui.graphics.StrokeCap.Round)
            }
        }
    }
}

@Composable
private fun HomeContent(onStream: (String?) -> Unit, navTo: (Tab) -> Unit) {
    val console = ConsoleStore.console
    val offline = dev.pulseplay.net.NetworkMonitor.offline()
    // Real games pulled from the console (only when it's paired and reachable). No
    // console / offline / can't-load → the strip is simply hidden, never faked.
    var games by remember { mutableStateOf<List<HelperClient.Game>?>(null) }
    LaunchedEffect(console, offline) {
        if (console == null || offline) { games = null; return@LaunchedEffect }
        games = try { withContext(Dispatchers.IO) { HelperClient.games(console.host, console.token) } } catch (e: Exception) { null }
    }
    Column(Modifier.fillMaxSize().verticalScroll(rememberScrollState())) {
        // Hero
        Row(Modifier.fillMaxWidth().clip(RoundedCornerShape(22.dp))
            .background(Brush.horizontalGradient(listOf(Color(0xFF0E1A2E), Ink))).padding(28.dp),
            verticalAlignment = Alignment.CenterVertically) {
            Column(Modifier.weight(1f)) {
                Text("PS5 Remote Play", color = Color.White, fontSize = 30.sp, fontWeight = FontWeight.Bold)
                Text("Stream and play your PS5 anywhere.", color = Muted, fontSize = 14.sp,
                    modifier = Modifier.padding(top = 6.dp, bottom = 20.dp))
                Button(onClick = { onStream(null) }, colors = ButtonDefaults.buttonColors(containerColor = Accent),
                    shape = RoundedCornerShape(13.dp), modifier = Modifier.height(50.dp)) {
                    Text("▶", color = Color.White, fontSize = 15.sp)
                    Spacer(Modifier.width(9.dp))
                    Text(if (console != null) "Connect to PS5" else "Pair your PS5", color = Color.White, fontSize = 16.sp, fontWeight = FontWeight.SemiBold)
                }
            }
            Ps5Art(Modifier.size(width = 150.dp, height = 116.dp))
        }
        Spacer(Modifier.height(16.dp))
        // Two info cards
        Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(14.dp)) {
            Row(Modifier.weight(1f).clip(RoundedCornerShape(16.dp)).clickable { navTo(Tab.CONSOLES) }.background(Card).padding(16.dp), verticalAlignment = Alignment.CenterVertically) {
                Box(Modifier.size(38.dp).clip(RoundedCornerShape(10.dp)).background(Ink).padding(8.dp)) { LogoMark(Modifier.fillMaxSize(), Accent) }
                Spacer(Modifier.width(12.dp))
                Column(Modifier.weight(1f)) {
                    Text("My PS5", color = OnDark, fontSize = 15.sp, fontWeight = FontWeight.SemiBold)
                    Row(verticalAlignment = Alignment.CenterVertically) {
                        Box(Modifier.size(7.dp).clip(RoundedCornerShape(50)).background(if (console == null) Muted else consoleDotColor(ConsoleStatus.state)))
                        Spacer(Modifier.width(6.dp))
                        Text(if (console == null) "Not paired" else ConsoleStatus.label(),
                            color = if (console != null && ConsoleStatus.state == ConsoleStatus.State.OFFLINE) Color(0xFFF85149) else Muted, fontSize = 12.sp)
                    }
                }
                Text("›", color = Muted, fontSize = 18.sp)
            }
            Row(Modifier.weight(1f).clip(RoundedCornerShape(16.dp)).clickable { navTo(Tab.SETTINGS) }.background(Card).padding(16.dp), verticalAlignment = Alignment.CenterVertically) {
                Box(Modifier.size(38.dp).clip(RoundedCornerShape(10.dp)).background(Accent.copy(alpha = 0.18f)), contentAlignment = Alignment.Center) {
                    Text("HD", color = Accent, fontSize = 13.sp, fontWeight = FontWeight.Bold)
                }
                Spacer(Modifier.width(12.dp))
                Column(Modifier.weight(1f)) {
                    Text("Streaming Quality", color = OnDark, fontSize = 15.sp, fontWeight = FontWeight.SemiBold)
                    Text(StreamPrefs.summary(), color = Muted, fontSize = 12.sp, modifier = Modifier.padding(top = 3.dp))
                }
                Text("›", color = Muted, fontSize = 18.sp)
            }
        }
        val gs = games
        if (!gs.isNullOrEmpty()) {
            Spacer(Modifier.height(24.dp))
            Text("Your Games", color = OnDark, fontSize = 15.sp, fontWeight = FontWeight.SemiBold)
            Spacer(Modifier.height(12.dp))
            val palette = listOf(Color(0xFF243A5E), Color(0xFF4A2030), Color(0xFF1F4F5A), Color(0xFF3A2F5E), Color(0xFF5A3A1F), Color(0xFF20405E))
            LazyRow(horizontalArrangement = Arrangement.spacedBy(14.dp)) {
                items(gs.take(10)) { g ->
                    val c = palette[(g.id.hashCode() and 0x7fffffff) % palette.size]
                    Column(Modifier.width(120.dp).clickable { onStream(g.id) }) {   // tap → launch that game
                        Box(Modifier.width(120.dp).height(150.dp).clip(RoundedCornerShape(13.dp)).background(Brush.verticalGradient(listOf(c, Ink))))
                        Text(g.name, color = OnDark, fontSize = 12.sp, maxLines = 1, modifier = Modifier.padding(top = 6.dp))
                    }
                }
            }
        }
    }
}

/** A stylized PS5 console (two white fins, dark body, a blue light). */
@Composable
private fun Ps5Art(modifier: Modifier) {
    Box(modifier, contentAlignment = Alignment.Center) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Box(Modifier.width(24.dp).height(104.dp).clip(RoundedCornerShape(6.dp)).background(Brush.verticalGradient(listOf(Color.White, Color(0xFFD7DEEA)))))
            Box(Modifier.width(16.dp).height(104.dp).background(Color(0xFF090C12)), contentAlignment = Alignment.Center) {
                Box(Modifier.width(3.dp).height(72.dp).clip(RoundedCornerShape(2.dp)).background(Brush.verticalGradient(listOf(Accent, Color(0xFF66C0FF)))))
            }
            Box(Modifier.width(24.dp).height(104.dp).clip(RoundedCornerShape(6.dp)).background(Brush.verticalGradient(listOf(Color.White, Color(0xFFD7DEEA)))))
        }
    }
}

@Composable
private fun ConsolesContent(onStream: (String?) -> Unit, onPair: () -> Unit) {
    val console = ConsoleStore.console
    val ctx = androidx.compose.ui.platform.LocalContext.current
    Column(Modifier.fillMaxSize().verticalScroll(rememberScrollState())) {
        Text("Consoles", color = Color.White, fontSize = 24.sp, fontWeight = FontWeight.SemiBold)
        Spacer(Modifier.height(18.dp))
        if (console != null) {
            Row(Modifier.fillMaxWidth().clip(RoundedCornerShape(16.dp)).background(Card).padding(16.dp), verticalAlignment = Alignment.CenterVertically) {
                Ps5Art(Modifier.size(width = 56.dp, height = 56.dp))
                Spacer(Modifier.width(14.dp))
                Column(Modifier.weight(1f)) {
                    Text("My PS5", color = OnDark, fontSize = 16.sp, fontWeight = FontWeight.SemiBold)
                    Row(verticalAlignment = Alignment.CenterVertically) {
                        Box(Modifier.size(7.dp).clip(RoundedCornerShape(50)).background(consoleDotColor(ConsoleStatus.state)))
                        Spacer(Modifier.width(6.dp))
                        Text("${console.host} · ${ConsoleStatus.label()}",
                            color = if (ConsoleStatus.state == ConsoleStatus.State.OFFLINE) Color(0xFFF85149) else Muted, fontSize = 12.sp)
                    }
                    ConsoleStatus.runningApp?.takeIf { it.isNotBlank() && ConsoleStatus.state == ConsoleStatus.State.ONLINE }?.let { app ->
                        Text("Playing $app", color = Accent, fontSize = 11.sp, modifier = Modifier.padding(top = 2.dp))
                    }
                }
                Button(onClick = {
                    Discovery.wake(console)
                    android.widget.Toast.makeText(ctx, "Wake signal sent to your PS5", android.widget.Toast.LENGTH_SHORT).show()
                }, colors = ButtonDefaults.buttonColors(containerColor = Color(0x22FFFFFF)),
                    shape = RoundedCornerShape(12.dp), modifier = Modifier.height(44.dp)) { Text("Wake", color = OnDark) }
                Spacer(Modifier.width(8.dp))
                Button(onClick = { onStream(null) }, colors = ButtonDefaults.buttonColors(containerColor = Accent),
                    shape = RoundedCornerShape(12.dp), modifier = Modifier.height(44.dp)) { Text("Play", color = Color.White, fontWeight = FontWeight.SemiBold) }
            }
            Spacer(Modifier.height(14.dp))
        }
        Column(Modifier.fillMaxWidth().clip(RoundedCornerShape(16.dp)).background(Color.White.copy(alpha = 0.03f))) {
            // onPair → the pair screen directly. (onStream(null) would start a *stream*
            // whenever a console was already paired, so it could never reach pairing.)
            SettingRow("Pair a new PS5", "", onClick = onPair)
            SettingRow(if (console != null) "Forget this PS5" else "Nothing paired yet", "",
                onClick = if (console != null) ({ runCatching { AwayManager.disconnect(); ConsoleStore.forget(); ConsoleStore.forgetLink() } }) else null)
        }
    }
}

@Composable
private fun ControllerContent() {
    var controls by remember { mutableStateOf(StreamPrefs.alwaysShowControls) }
    var opacity by remember { mutableStateOf(StreamPrefs.opacityLabel()) }
    Column(Modifier.fillMaxSize().verticalScroll(rememberScrollState())) {
        Text("Controller", color = Color.White, fontSize = 24.sp, fontWeight = FontWeight.SemiBold)
        Spacer(Modifier.height(18.dp))
        Text("ON-SCREEN CONTROLS", color = Muted, fontSize = 11.sp, fontWeight = FontWeight.SemiBold, modifier = Modifier.padding(start = 4.dp, bottom = 10.dp))
        Column(Modifier.fillMaxWidth().clip(RoundedCornerShape(16.dp)).background(Color.White.copy(alpha = 0.03f))) {
            ToggleRow("Always show on-screen controls", controls) { StreamPrefs.toggleControls(); controls = StreamPrefs.alwaysShowControls }
            SettingRow("On-screen controls opacity", opacity, onClick = { StreamPrefs.cycleOpacity(); opacity = StreamPrefs.opacityLabel() })
            SettingRow("Edit layout", "Coming soon", soon = true)
            SettingRow("Button mapping", "Coming soon", soon = true)
        }
        Spacer(Modifier.height(18.dp))
        Text("DUALSENSE", color = Muted, fontSize = 11.sp, fontWeight = FontWeight.SemiBold, modifier = Modifier.padding(start = 4.dp, bottom = 10.dp))
        Column(Modifier.fillMaxWidth().clip(RoundedCornerShape(16.dp)).background(Color.White.copy(alpha = 0.03f))) {
            SettingRow("Vibration (Haptic)", "Coming soon", soon = true)
            SettingRow("Adaptive Triggers", "Coming soon", soon = true)
            SettingRow("Gyro aiming", "Coming soon", soon = true)
        }
    }
}

@Composable
private fun AboutContent() {
    Column(Modifier.fillMaxSize().verticalScroll(rememberScrollState())) {
        Text("About", color = Color.White, fontSize = 24.sp, fontWeight = FontWeight.SemiBold)
        Spacer(Modifier.height(18.dp))
        Row(verticalAlignment = Alignment.CenterVertically) {
            Box(Modifier.size(44.dp).clip(RoundedCornerShape(12.dp)).background(Brush.linearGradient(listOf(Accent, Color(0xFF0B4EA0)))).padding(9.dp)) { LogoMark(Modifier.fillMaxSize(), Color.White) }
            Spacer(Modifier.width(12.dp))
            Column { Text("PulsePlay", color = Color.White, fontSize = 18.sp, fontWeight = FontWeight.Bold); Text("v1.0.0 · AGPL-3.0", color = Muted, fontSize = 12.sp) }
        }
        Spacer(Modifier.height(16.dp))
        Text("Made by Jxswl606", color = OnDark, fontSize = 15.sp, fontWeight = FontWeight.SemiBold)
        Spacer(Modifier.height(18.dp))
        Text("Free, open-source Remote Play for jailbroken PS5s — no PSN. Built on the work of:",
            color = Muted, fontSize = 13.sp, modifier = Modifier.padding(bottom = 14.dp))
        Column(Modifier.fillMaxWidth().clip(RoundedCornerShape(16.dp)).background(Color.White.copy(alpha = 0.03f))) {
            SettingRow("chiaki-ng", "streetpea")
            SettingRow("Chiaki", "thestr4ng3r")
            SettingRow("Orbit Play", "saawant12")
            SettingRow("ps5-remoteplay-get-pin", "idlesauce")
            SettingRow("PS5 Payload SDK", "John Törnblom")
            SettingRow("PS5 homebrew community", "")
        }
    }
}

@Composable
private fun ToggleRow(label: String, on: Boolean, onToggle: () -> Unit) {
    Row(Modifier.fillMaxWidth().clickable(onClick = onToggle).padding(horizontal = 18.dp, vertical = 15.dp), verticalAlignment = Alignment.CenterVertically) {
        Text(label, color = OnDark, fontSize = 15.sp, fontWeight = FontWeight.Medium)
        Spacer(Modifier.weight(1f))
        Box(Modifier.size(width = 44.dp, height = 26.dp).clip(RoundedCornerShape(50)).background(if (on) Accent else Color(0x24FFFFFF)),
            contentAlignment = if (on) Alignment.CenterEnd else Alignment.CenterStart) {
            Box(Modifier.padding(3.dp).size(20.dp).clip(RoundedCornerShape(50)).background(Color.White))
        }
    }
}

@Composable
private fun LibraryContent(onStream: (String?) -> Unit) {
    val console = ConsoleStore.console
    var games by remember { mutableStateOf<List<HelperClient.Game>?>(null) }
    var error by remember { mutableStateOf<String?>(null) }
    LaunchedEffect(console) {
        if (console == null) return@LaunchedEffect
        try { games = withContext(Dispatchers.IO) { HelperClient.games(console.host, console.token) } }
        catch (e: Exception) { error = e.message ?: "Could not load games." }
    }
    Column(Modifier.fillMaxSize()) {
        Text("Library", color = Color.White, fontSize = 24.sp, fontWeight = FontWeight.SemiBold)
        Spacer(Modifier.height(16.dp))
        when {
            console == null -> Text("Pair your PS5 first to see your games.", color = Muted, fontSize = 14.sp)
            error != null -> Text(error!!, color = Color(0xFFF85149), fontSize = 14.sp)
            games == null -> Text("Loading your games…", color = Muted, fontSize = 14.sp)
            games!!.isEmpty() -> Text("No games found on the console.", color = Muted, fontSize = 14.sp)
            else -> {
                val palette = listOf(Color(0xFF243A5E), Color(0xFF4A2030), Color(0xFF1F4F5A), Color(0xFF3A2F5E), Color(0xFF5A3A1F), Color(0xFF20405E))
                LazyVerticalGrid(columns = GridCells.Adaptive(110.dp),
                    horizontalArrangement = Arrangement.spacedBy(12.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
                    gridItems(games!!) { g ->
                        val c = palette[(g.id.hashCode() and 0x7fffffff) % palette.size]
                        Column(Modifier.clickable { onStream(g.id) }) {
                            Box(Modifier.aspectRatio(0.75f).clip(RoundedCornerShape(12.dp)).background(Brush.verticalGradient(listOf(c, Bg0))))
                            Text(g.name, color = OnDark, fontSize = 12.sp, maxLines = 2, modifier = Modifier.padding(top = 6.dp))
                        }
                    }
                }
            }
        }
    }
}

@Composable
private fun SettingsContent(onForget: () -> Unit = {}) {
    val console = ConsoleStore.console
    var presetSel by remember { mutableStateOf(StreamPrefs.currentPreset()) }
    var resSel by remember { mutableStateOf(StreamPrefs.res) }
    var fpsSel by remember { mutableStateOf(StreamPrefs.fps) }
    var bw by remember { mutableStateOf(StreamPrefs.bitrateLabel()) }
    var controls by remember { mutableStateOf(StreamPrefs.controlsLabel()) }
    var opacity by remember { mutableStateOf(StreamPrefs.opacityLabel()) }
    var sticks by remember { mutableStateOf(StreamPrefs.sticksLabel()) }
    var fillSel by remember { mutableStateOf(StreamPrefs.fillMode) }
    val scope = rememberCoroutineScope()
    var testing by remember { mutableStateOf(false) }
    var testResult by remember { mutableStateOf<NetworkTest.Result?>(null) }
    fun sync() { presetSel = StreamPrefs.currentPreset(); resSel = StreamPrefs.res; fpsSel = StreamPrefs.fps; bw = StreamPrefs.bitrateLabel() }

    val cardMod = Modifier.fillMaxWidth().clip(RoundedCornerShape(16.dp)).background(Color.White.copy(alpha = 0.03f))
    Column(Modifier.fillMaxSize().verticalScroll(rememberScrollState())) {
        Text("Settings", color = Color.White, fontSize = 24.sp, fontWeight = FontWeight.SemiBold)
        Text("Tune how PulsePlay streams from your PS5.", color = Muted, fontSize = 13.sp, modifier = Modifier.padding(top = 4.dp))

        GroupLabel("STREAMING")
        Column(cardMod) {
            SegmentedRow("Preset", listOf("Performance", "Balanced", "Quality"),
                when (presetSel) { Preset.PERFORMANCE -> 0; Preset.BALANCED -> 1; Preset.QUALITY -> 2; else -> -1 }) { i ->
                StreamPrefs.applyPreset(listOf(Preset.PERFORMANCE, Preset.BALANCED, Preset.QUALITY)[i]); sync()
            }
            SegmentedRow("Resolution", Res.values().map { it.label }, Res.values().indexOf(resSel)) { i -> StreamPrefs.applyRes(Res.values()[i]); sync() }
            SegmentedRow("Frame rate", Fps.values().map { it.label }, Fps.values().indexOf(fpsSel)) { i -> StreamPrefs.applyFps(Fps.values()[i]); sync() }
            StepperRow("Bandwidth", bw, onMinus = { StreamPrefs.stepBitrate(false); sync() }, onPlus = { StreamPrefs.stepBitrate(true); sync() })
            SegmentedRow("Display", FillMode.values().map { it.label }, FillMode.values().indexOf(fillSel)) { i ->
                StreamPrefs.applyFillMode(FillMode.values()[i]); fillSel = FillMode.values()[i]
            }
        }

        GroupLabel("NETWORK")
        Column(cardMod) {
            SettingRow(
                if (testing) "Testing…" else "Test network",
                if (testing) "" else (testResult?.let { "${it.grade} · ${it.avgMs} ms" } ?: "Measure latency"),
                onClick = if (console != null && !testing) ({
                    val host = console.host
                    testing = true; testResult = null
                    scope.launch { testResult = NetworkTest.run(host); testing = false }
                }) else null,
            )
            testResult?.let { r -> NetworkResult(r) { StreamPrefs.applyPreset(r.recommend); sync() } }
        }

        GroupLabel("CONTROLS")
        Column(cardMod) {
            SettingRow("On-screen sticks", sticks, onClick = { StreamPrefs.toggleDynamicSticks(); sticks = StreamPrefs.sticksLabel() })
            SettingRow("Controller & touch", controls, onClick = { StreamPrefs.toggleControls(); controls = StreamPrefs.controlsLabel() })
            SettingRow("On-screen controls opacity", opacity, onClick = { StreamPrefs.cycleOpacity(); opacity = StreamPrefs.opacityLabel() })
        }

        GroupLabel("GENERAL")
        Column(cardMod) {
            SettingRow("Microphone", "Coming soon", soon = true)
            AwaySetting()
        }

        // Only offer "Forget" when a console is actually paired.
        if (console != null) {
            Spacer(Modifier.height(18.dp))
            TextButton(onClick = {
                runCatching { AwayManager.disconnect(); ConsoleStore.forget(); ConsoleStore.forgetLink() }
                onForget()
            }) { Text("Forget this PS5", color = Color(0xFFF85149), fontSize = 14.sp) }
        }
    }
}

@Composable
private fun GroupLabel(text: String) {
    Text(text, color = Muted, fontSize = 11.sp, fontWeight = FontWeight.SemiBold,
        modifier = Modifier.padding(start = 4.dp, top = 18.dp, bottom = 10.dp))
}

/** Network-test results: metric chips + the recommended preset with an Apply button. */
@Composable
private fun NetworkResult(r: NetworkTest.Result, onApply: () -> Unit) {
    Column(Modifier.fillMaxWidth().padding(horizontal = 16.dp).padding(bottom = 14.dp)) {
        if (!r.reachable) {
            Text(r.note, color = Color(0xFFF0A05A), fontSize = 13.sp)
        } else {
            Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                Metric("Grade", r.grade, Modifier.weight(1f))
                Metric("Latency", "${r.avgMs} ms", Modifier.weight(1f))
                Metric("Jitter", "${r.jitterMs} ms", Modifier.weight(1f))
                Metric("Loss", "${r.lossPct}%", Modifier.weight(1f))
            }
            Spacer(Modifier.height(10.dp))
            Text("${r.connection} · ${r.note}", color = Muted, fontSize = 12.sp)
            Spacer(Modifier.height(10.dp))
            val rec = when (r.recommend) {
                Preset.PERFORMANCE -> "Performance"; Preset.BALANCED -> "Balanced"
                Preset.QUALITY -> "Quality"; else -> "Custom"
            }
            Row(verticalAlignment = Alignment.CenterVertically) {
                Text("Recommended: $rec", color = OnDark, fontSize = 13.sp, fontWeight = FontWeight.Medium)
                Spacer(Modifier.weight(1f))
                Box(Modifier.clip(RoundedCornerShape(50)).background(Accent).clickable(onClick = onApply)
                    .padding(horizontal = 18.dp, vertical = 8.dp)) {
                    Text("Apply", color = Color.White, fontSize = 13.sp, fontWeight = FontWeight.Medium)
                }
            }
        }
    }
}

@Composable
private fun Metric(label: String, value: String, modifier: Modifier = Modifier) {
    Column(
        modifier.clip(RoundedCornerShape(10.dp)).background(Color(0x14FFFFFF)).padding(vertical = 10.dp, horizontal = 6.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        Text(value, color = OnDark, fontSize = 14.sp, fontWeight = FontWeight.SemiBold, maxLines = 1)
        Text(label, color = Muted, fontSize = 10.sp, modifier = Modifier.padding(top = 2.dp))
    }
}

/** A horizontal segmented picker — tap any option directly (moves both ways). */
@Composable
private fun SegmentedRow(label: String, options: List<String>, selected: Int, onSelect: (Int) -> Unit) {
    Column(Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 13.dp)) {
        Text(label, color = OnDark, fontSize = 14.sp, fontWeight = FontWeight.Medium, modifier = Modifier.padding(bottom = 9.dp))
        Row(Modifier.fillMaxWidth().clip(RoundedCornerShape(10.dp)).background(Color(0x14FFFFFF)).padding(3.dp),
            horizontalArrangement = Arrangement.spacedBy(3.dp)) {
            options.forEachIndexed { i, opt ->
                Box(Modifier.weight(1f).clip(RoundedCornerShape(8.dp))
                    .background(if (i == selected) Accent else Color.Transparent)
                    .clickable { onSelect(i) }.padding(vertical = 9.dp), contentAlignment = Alignment.Center) {
                    Text(opt, color = if (i == selected) Color.White else Muted, fontSize = 13.sp, fontWeight = FontWeight.Medium)
                }
            }
        }
    }
}

/** A −/value/+ stepper for bandwidth (decrease left, increase right). */
@Composable
private fun StepperRow(label: String, value: String, onMinus: () -> Unit, onPlus: () -> Unit) {
    Row(Modifier.fillMaxWidth().padding(horizontal = 18.dp, vertical = 14.dp), verticalAlignment = Alignment.CenterVertically) {
        Text(label, color = OnDark, fontSize = 15.sp, fontWeight = FontWeight.Medium)
        Spacer(Modifier.weight(1f))
        StepBtn("−", onMinus)
        Text(value, color = OnDark, fontSize = 14.sp, textAlign = androidx.compose.ui.text.style.TextAlign.Center, modifier = Modifier.width(84.dp))
        StepBtn("+", onPlus)
    }
}

@Composable
private fun StepBtn(label: String, onClick: () -> Unit) {
    Box(Modifier.size(34.dp).clip(RoundedCornerShape(9.dp)).background(Color(0x1FFFFFFF)).clickable(onClick = onClick),
        contentAlignment = Alignment.Center) { Text(label, color = Accent, fontSize = 19.sp, fontWeight = FontWeight.Bold) }
}

/**
 * "Play away from home" via Tailscale. This build streams over any network as long as
 * the PS5 is reachable by IP, so there's no in-app tunnel to configure: put the phone
 * and the PS5's network on the same Tailscale tailnet, then pair/connect with the PS5's
 * Tailscale IP exactly like at home. Tapping the row just explains how.
 */
@Composable
private fun AwaySetting() {
    var expanded by remember { mutableStateOf(false) }
    Column {
        SettingRow("Play away from home", "Tailscale", onClick = { expanded = !expanded })
        if (expanded) {
            Text(
                "Install Tailscale on this phone and expose your PS5's network to your tailnet " +
                    "(a subnet route on a home device, or Tailscale on the console's router). " +
                    "Then pair and connect using the PS5's Tailscale IP (100.x.y.z) — Remote Play " +
                    "works anywhere, just like on your home Wi-Fi. Keep the PS5 on with the helper running.",
                color = Muted, fontSize = 12.sp,
                modifier = Modifier.padding(start = 18.dp, end = 18.dp, bottom = 14.dp),
            )
        }
    }
}

@Composable
private fun SettingRow(label: String, value: String, soon: Boolean = false, onClick: (() -> Unit)? = null) {
    val clickable = onClick != null && !soon
    val m = if (clickable) Modifier.fillMaxWidth().clickable(onClick = onClick!!) else Modifier.fillMaxWidth()
    Row(m.padding(horizontal = 18.dp, vertical = 17.dp), verticalAlignment = Alignment.CenterVertically) {
        Text(label, color = if (soon) Muted else OnDark, fontSize = 15.sp, fontWeight = FontWeight.Medium)
        Spacer(Modifier.weight(1f))
        Text(value, color = if (soon) Color(0xFF586375) else if (clickable) Accent else Muted, fontSize = 13.sp)
        if (clickable) {
            Spacer(Modifier.width(8.dp))
            Text("›", color = Muted, fontSize = 18.sp)        // tappable-row arrow pointer
        }
    }
}

@Composable
private fun PairScreen(onBack: () -> Unit, onPaired: () -> Unit) {
    val scope = rememberCoroutineScope()
    var ip by remember { mutableStateOf(TextFieldValue("10.0.0.")) }
    var code by remember { mutableStateOf(TextFieldValue("")) }
    var busy by remember { mutableStateOf(false) }
    var status by remember { mutableStateOf("") }
    var ok by remember { mutableStateOf(false) }

    // Android's back gesture leaves the pair screen.
    androidx.activity.compose.BackHandler(enabled = !busy) { onBack() }

    // safeDrawingPadding keeps content clear of the status bar and the gesture pill;
    // verticalScroll means nothing is cut off on short landscape screens.
    Column(
        Modifier.fillMaxSize().safeDrawingPadding().verticalScroll(rememberScrollState())
            .padding(horizontal = 28.dp, vertical = 16.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        // Top bar with a visible back arrow.
        Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
            TextButton(onClick = onBack, enabled = !busy) { Text("‹ Back", color = Muted, fontSize = 15.sp) }
            Spacer(Modifier.weight(1f))
        }
        Spacer(Modifier.height(4.dp))
        Text("Pair with your PS5", color = Color.White, fontSize = 24.sp, fontWeight = FontWeight.Bold)
        Text("Run the Orbit Play helper on your PS5, then enter its local IP and the 6-digit code it shows.",
            color = Muted, fontSize = 13.sp, textAlign = androidx.compose.ui.text.style.TextAlign.Center,
            modifier = Modifier.padding(top = 8.dp, bottom = 18.dp))
        OutlinedTextField(value = ip, onValueChange = { ip = it }, label = { Text("PS5 IP address") },
            singleLine = true, keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Number), modifier = Modifier.fillMaxWidth())
        OutlinedTextField(value = code, onValueChange = { if (it.text.length <= 6) code = it }, label = { Text("6-digit code") },
            singleLine = true, keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Number), modifier = Modifier.fillMaxWidth().padding(top = 12.dp))
        TextButton(enabled = !busy, onClick = {
            busy = true; ok = false; status = "Searching for your PS5…"
            scope.launch {
                val found = Discovery.findPs5()
                if (found != null) { ip = TextFieldValue(found); status = "Found $found" }
                else status = "No PS5 found automatically — enter the IP above."
                busy = false
            }
        }, modifier = Modifier.padding(top = 4.dp)) { Text("Find my PS5 automatically", color = Accent, fontSize = 13.sp) }
        Button(enabled = !busy, onClick = {
            status = ""; busy = true
            scope.launch {
                try {
                    val host = ip.text.trim()
                    val token = withContext(Dispatchers.IO) { HelperClient.unlock(host, code.text.trim()) }
                    Registration.register(host, token)
                    ok = true; status = "Registered! Starting stream…"
                    onPaired()                       // go straight into the stream
                } catch (e: HelperError) { ok = false; status = e.message ?: "Pairing failed." }
                catch (e: Exception) { ok = false; status = e.message ?: "Pairing failed." }
                finally { busy = false }
            }
        }, colors = ButtonDefaults.buttonColors(containerColor = Accent), shape = RoundedCornerShape(14.dp),
            modifier = Modifier.fillMaxWidth().height(54.dp).padding(top = 20.dp)) {
            Text(if (busy) "Connecting…" else "Connect", color = Color.White, fontSize = 17.sp)
        }
        if (status.isNotEmpty()) Text(status, color = if (ok) Good else Color(0xFFF85149), fontSize = 14.sp,
            textAlign = androidx.compose.ui.text.style.TextAlign.Center, modifier = Modifier.padding(top = 16.dp))
        Spacer(Modifier.height(12.dp))
    }
}

@Composable
private fun StreamScreen(onExit: () -> Unit, onReconnect: () -> Unit) {
    val console = ConsoleStore.console
    if (console == null) { onExit(); return }
    val activity = androidx.compose.ui.platform.LocalContext.current as? android.app.Activity
    var status by remember { mutableStateOf("Starting…") }
    var quit by remember { mutableStateOf(false) }
    val appCtx = androidx.compose.ui.platform.LocalContext.current.applicationContext
    val session = remember { StreamSession(appCtx, console, { status = it }, { reason -> status = reason; quit = true }) }
    var barVisible by remember { mutableStateOf(false) }
    var padHidden by remember { mutableStateOf(false) }  // Controls button hides the on-screen pad
    var poke by remember { mutableStateOf(0) }           // bump to restart the auto-hide timer
    var pingMs by remember { mutableStateOf(-1) }
    // Live latency readout: ping the console every ~2s while connected (the engine
    // exposes no FPS/bitrate, so latency is the honest live signal).
    LaunchedEffect(status) {
        if (status != "Connected") return@LaunchedEffect
        while (true) {
            pingMs = withContext(Dispatchers.IO) { NetworkTest.pingOnce(console.host) }
            delay(2000)
        }
    }
    // Auto-hide the control bar a few seconds after it's shown or last touched.
    LaunchedEffect(barVisible, poke) { if (barVisible) { delay(4000); barVisible = false } }
    // When the stream ends (or drops), show the reason briefly then return Home — no hang.
    LaunchedEffect(quit) { if (quit) { delay(900); onExit() } }
    // Back gesture toggles the control bar instead of killing the session outright.
    androidx.activity.compose.BackHandler { barVisible = !barVisible }
    // End the stream when the app goes to the background — don't keep streaming (and
    // burning battery/data) blind. Returning to the app starts a fresh connection.
    DisposableEffect(Unit) {
        val owner = activity as? androidx.lifecycle.LifecycleOwner
        val obs = androidx.lifecycle.LifecycleEventObserver { _, e ->
            if (e == androidx.lifecycle.Lifecycle.Event.ON_STOP) onExit()
        }
        owner?.lifecycle?.addObserver(obs)
        onDispose { owner?.lifecycle?.removeObserver(obs) }
    }
    DisposableEffect(Unit) {
        // Keep the screen awake and lock to landscape while streaming.
        activity?.window?.addFlags(android.view.WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
        val prevOrientation = activity?.requestedOrientation
        activity?.requestedOrientation = android.content.pm.ActivityInfo.SCREEN_ORIENTATION_SENSOR_LANDSCAPE
        // Immersive fullscreen: hide the status/navigation bars for the whole session.
        val controller = activity?.window?.let {
            androidx.core.view.WindowInsetsControllerCompat(it, it.decorView).apply {
                systemBarsBehavior = androidx.core.view.WindowInsetsControllerCompat.BEHAVIOR_SHOW_TRANSIENT_BARS_BY_SWIPE
            }
        }
        controller?.hide(androidx.core.view.WindowInsetsCompat.Type.systemBars())
        ActiveStream.session = session
        session.start()
        onDispose {
            // Stop routing input first, then tear down the native session.
            ActiveStream.clear()
            session.stop()
            // Leave the system bars hidden (app-wide immersive); just drop keep-screen-on.
            activity?.window?.clearFlags(android.view.WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
            prevOrientation?.let { activity.requestedOrientation = it }
        }
    }
    // Stretch to fill the whole screen (default) or keep 16:9 with black bars.
    val videoMod = if (StreamPrefs.fillMode == FillMode.FIT) Modifier.aspectRatio(16f / 9f) else Modifier.fillMaxSize()
    Box(Modifier.fillMaxSize().background(Color.Black), contentAlignment = Alignment.Center) {
        AndroidView(factory = { ctx ->
            SurfaceView(ctx).apply {
                holder.addCallback(object : SurfaceHolder.Callback {
                    // Attach in surfaceChanged (valid dimensions), detach in surfaceDestroyed —
                    // exactly how chiaki-ng drives the decoder's surface.
                    override fun surfaceCreated(h: SurfaceHolder) {}
                    override fun surfaceChanged(h: SurfaceHolder, f: Int, w: Int, ht: Int) { session.setSurface(h.surface) }
                    override fun surfaceDestroyed(h: SurfaceHolder) { session.setSurface(null) }
                })
            }
        }, modifier = videoMod)

        // On-screen controls: shown unless the user hid them (Controls button), and
        // auto-hidden once a physical controller is in use (unless forced always-on).
        if (!padHidden && (StreamPrefs.alwaysShowControls || !ActiveStream.hasGamepadState.value)) GamepadOverlay()

        // Connection feedback, centered, only until the first frame arrives.
        if (status != "Connected") {
            Text(status, color = Color.White, fontSize = 15.sp, modifier = Modifier.align(Alignment.Center))
        }

        // Live connection chip, top-left (dot + latency + network).
        StreamStatusChip(status, pingMs, Modifier.align(Alignment.TopStart).padding(14.dp))

        // Bottom control bar, slides in on reveal (tap anywhere on the video to show it).
        androidx.compose.animation.AnimatedVisibility(
            visible = barVisible,
            modifier = Modifier.align(Alignment.BottomCenter).padding(bottom = 20.dp),
        ) {
            StreamControlBar(
                onQuality = { StreamPrefs.applyPreset(nextPreset(StreamPrefs.currentPreset())); onReconnect() },
                onControls = { padHidden = !padHidden; poke++ },
                onDisplay = {
                    StreamPrefs.applyFillMode(if (StreamPrefs.fillMode == FillMode.STRETCH) FillMode.FIT else FillMode.STRETCH)
                    poke++
                },
                onDisconnect = onExit,
            )
        }

        // Small visible handle to open the control bar. A real, self-contained button —
        // it only captures its own area, so (unlike a full-screen tap-catcher) it can
        // never block the game controls. Back also toggles the bar.
        Box(
            Modifier.align(Alignment.TopCenter).padding(top = 8.dp)
                .clip(RoundedCornerShape(50)).background(Color(0x59000000))
                .clickable { barVisible = !barVisible; poke++ }
                .padding(horizontal = 20.dp, vertical = 5.dp),
            contentAlignment = Alignment.Center,
        ) {
            Text("•  •  •", color = Color(0xB3FFFFFF), fontSize = 13.sp, fontWeight = FontWeight.Bold)
        }
    }
}

private enum class OverlayGlyph { QUALITY, CONTROLS, DISPLAY, POWER }

private fun nextPreset(p: Preset): Preset = when (p) {
    Preset.PERFORMANCE -> Preset.BALANCED
    Preset.BALANCED -> Preset.QUALITY
    else -> Preset.PERFORMANCE   // QUALITY or CUSTOM wrap to Performance
}

@Composable
private fun StreamStatusChip(status: String, pingMs: Int, modifier: Modifier) {
    val connected = status == "Connected"
    val net = dev.pulseplay.net.NetworkMonitor.label()
    val dot = when {
        !connected || pingMs < 0 -> Muted
        pingMs < 40 -> Good
        pingMs < 100 -> Color(0xFFE0B24A)
        else -> Color(0xFFF0A05A)
    }
    Row(
        modifier.alpha(0.75f).clip(RoundedCornerShape(50)).background(Color(0x80000000)).padding(horizontal = 12.dp, vertical = 7.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Box(Modifier.size(7.dp).clip(RoundedCornerShape(50)).background(dot))
        Spacer(Modifier.width(8.dp))
        Text(
            if (!connected) status else if (pingMs < 0) "Connected · $net" else "$pingMs ms · $net",
            color = Color(0xE6FFFFFF), fontSize = 12.sp, fontWeight = FontWeight.Medium,
        )
    }
}

@Composable
private fun StreamControlBar(onQuality: () -> Unit, onControls: () -> Unit, onDisplay: () -> Unit, onDisconnect: () -> Unit) {
    Row(
        Modifier.clip(RoundedCornerShape(20.dp)).background(Color(0xDB0E131D)).padding(8.dp),
        horizontalArrangement = Arrangement.spacedBy(2.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        OverlayButton(OverlayGlyph.QUALITY, "Quality", false, onQuality)
        OverlayButton(OverlayGlyph.CONTROLS, "Controls", false, onControls)
        OverlayButton(OverlayGlyph.DISPLAY, "Display", false, onDisplay)
        Box(Modifier.width(1.dp).height(34.dp).background(Color(0x1AFFFFFF)))
        OverlayButton(OverlayGlyph.POWER, "Disconnect", true, onDisconnect)
    }
}

@Composable
private fun OverlayButton(glyph: OverlayGlyph, label: String, danger: Boolean, onClick: () -> Unit) {
    val interaction = remember { MutableInteractionSource() }
    val pressed by interaction.collectIsPressedAsState()
    // Clear highlight on the item you're touching, and bright labels so you can
    // actually read them over the video.
    val accent = if (danger) Color(0xFFFF6B6B) else OnDark
    Column(
        Modifier.clip(RoundedCornerShape(14.dp))
            .background(if (pressed) (if (danger) Color(0x40FF6B6B) else Color(0x40FFFFFF)) else Color.Transparent)
            .clickable(interactionSource = interaction, indication = null, onClick = onClick)
            .padding(horizontal = 14.dp, vertical = 10.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        OverlayIcon(glyph, accent)
        Spacer(Modifier.height(5.dp))
        Text(label, color = if (danger) Color(0xFFFF9A9A) else Color(0xF2FFFFFF), fontSize = 12.sp, fontWeight = FontWeight.SemiBold)
    }
}

@Composable
private fun OverlayIcon(glyph: OverlayGlyph, tint: Color) {
    androidx.compose.foundation.Canvas(Modifier.size(23.dp)) {
        val s = size.minDimension
        val st = s * 0.085f
        val cap = androidx.compose.ui.graphics.StrokeCap.Round
        val stroke = androidx.compose.ui.graphics.drawscope.Stroke(width = st, cap = cap, join = androidx.compose.ui.graphics.StrokeJoin.Round)
        fun o(x: Float, y: Float) = androidx.compose.ui.geometry.Offset(s * x, s * y)
        when (glyph) {
            OverlayGlyph.QUALITY -> {   // sliders / equalizer
                drawLine(tint, o(0.14f, 0.34f), o(0.86f, 0.34f), strokeWidth = st, cap = cap)
                drawLine(tint, o(0.14f, 0.66f), o(0.86f, 0.66f), strokeWidth = st, cap = cap)
                drawCircle(tint, s * 0.1f, o(0.66f, 0.34f))
                drawCircle(tint, s * 0.1f, o(0.36f, 0.66f))
            }
            OverlayGlyph.CONTROLS -> {  // gamepad
                drawRoundRect(
                    tint, topLeft = o(0.08f, 0.33f),
                    size = androidx.compose.ui.geometry.Size(s * 0.84f, s * 0.34f),
                    cornerRadius = androidx.compose.ui.geometry.CornerRadius(s * 0.17f, s * 0.17f), style = stroke,
                )
                drawCircle(tint, s * 0.05f, o(0.3f, 0.5f))
                drawCircle(tint, s * 0.05f, o(0.7f, 0.5f))
            }
            OverlayGlyph.DISPLAY -> {   // screen / aspect
                drawRoundRect(
                    tint, topLeft = o(0.12f, 0.22f),
                    size = androidx.compose.ui.geometry.Size(s * 0.76f, s * 0.56f),
                    cornerRadius = androidx.compose.ui.geometry.CornerRadius(s * 0.1f, s * 0.1f), style = stroke,
                )
            }
            OverlayGlyph.POWER -> {     // power
                drawArc(
                    tint, -55f, 290f, false, topLeft = o(0.22f, 0.26f),
                    size = androidx.compose.ui.geometry.Size(s * 0.56f, s * 0.56f), style = stroke,
                )
                drawLine(tint, o(0.5f, 0.14f), o(0.5f, 0.48f), strokeWidth = st, cap = cap)
            }
        }
    }
}

