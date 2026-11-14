/*
 * PulsePlay — free, open-source Remote Play for jailbroken PS5s (no PSN).
 * Copyright (C) 2026 PulsePlay contributors. Licensed AGPL-3.0-only.
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
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyRow
import androidx.compose.foundation.lazy.grid.GridCells
import androidx.compose.foundation.lazy.grid.LazyVerticalGrid
import androidx.compose.foundation.lazy.grid.items as gridItems
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
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
import dev.pulseplay.stream.ActiveStream
import dev.pulseplay.stream.AwayManager
import dev.pulseplay.stream.ConsoleStore
import dev.pulseplay.stream.Discovery
import dev.pulseplay.stream.GamepadOverlay
import dev.pulseplay.stream.QualityPreset
import dev.pulseplay.stream.Registration
import dev.pulseplay.stream.StreamPrefs
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

private val Colors = darkColorScheme(
    primary = Accent, background = Bg0, surface = Panel,
    onBackground = OnDark, onSurface = OnDark,
)

private fun screenBg() = Brush.verticalGradient(listOf(Bg2, Bg1, Bg0))

private enum class Screen { SPLASH, HOME, PAIR, STREAM }
private enum class Tab { HOME, LIBRARY, SETTINGS }

class MainActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        ConsoleStore.init(this)
        StreamPrefs.init(this)
        enableEdgeToEdge()
        setContent {
            MaterialTheme(colorScheme = Colors) {
                var screen by remember { mutableStateOf(Screen.SPLASH) }
                Box(Modifier.fillMaxSize().background(screenBg())) {
                    when (screen) {
                        Screen.SPLASH -> SplashScreen(onDone = { screen = Screen.HOME })
                        Screen.HOME -> HomeScreen(onStream = { gid ->
                            ActiveStream.pendingGameId = gid
                            screen = if (ConsoleStore.console != null) Screen.STREAM else Screen.PAIR
                        })
                        Screen.PAIR -> PairScreen(onBack = { screen = Screen.HOME })
                        Screen.STREAM -> StreamScreen(onExit = { screen = Screen.HOME })
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
private fun HomeScreen(onStream: (String?) -> Unit) {
    var tab by remember { mutableStateOf(Tab.HOME) }
    Row(Modifier.fillMaxSize()) {
        Sidebar(tab) { tab = it }
        Box(Modifier.weight(1f).fillMaxHeight().padding(horizontal = 28.dp, vertical = 26.dp)) {
            when (tab) {
                Tab.HOME -> HomeContent(onStream)
                Tab.LIBRARY -> LibraryContent(onStream)
                Tab.SETTINGS -> SettingsContent()
            }
        }
    }
}

@Composable
private fun Sidebar(tab: Tab, onSelect: (Tab) -> Unit) {
    Column(Modifier.width(210.dp).fillMaxHeight().background(Color(0xFF0A0F1A).copy(alpha = 0.6f)).padding(16.dp)) {
        Row(verticalAlignment = Alignment.CenterVertically, modifier = Modifier.padding(start = 6.dp, bottom = 22.dp)) {
            Box(Modifier.size(30.dp).clip(RoundedCornerShape(9.dp)).background(Brush.linearGradient(listOf(Accent, Color(0xFF0B4EA0)))).padding(6.dp)) {
                LogoMark(Modifier.fillMaxSize(), Color.White)
            }
            Spacer(Modifier.width(10.dp))
            Row {
                Text("Pulse", color = Color.White, fontSize = 17.sp, fontWeight = FontWeight.SemiBold)
                Text("Play", color = Accent, fontSize = 17.sp, fontWeight = FontWeight.SemiBold)
            }
        }
        NavItem("Home", tab == Tab.HOME) { onSelect(Tab.HOME) }
        NavItem("Library", tab == Tab.LIBRARY) { onSelect(Tab.LIBRARY) }
        NavItem("Settings", tab == Tab.SETTINGS) { onSelect(Tab.SETTINGS) }
        Spacer(Modifier.weight(1f))
        Row(Modifier.fillMaxWidth().clip(RoundedCornerShape(12.dp)).background(Color.White.copy(alpha = 0.03f)).padding(12.dp),
            verticalAlignment = Alignment.CenterVertically) {
            Box(Modifier.size(9.dp).clip(RoundedCornerShape(5.dp)).background(if (ConsoleStore.console != null) Good else Muted))
            Spacer(Modifier.width(10.dp))
            Column {
                Text("PlayStation 5", color = OnDark, fontSize = 13.sp, fontWeight = FontWeight.SemiBold)
                Text(if (ConsoleStore.console != null) "Ready · LAN" else "Not paired", color = Muted, fontSize = 11.sp)
            }
        }
    }
}

@Composable
private fun NavItem(label: String, selected: Boolean, onClick: () -> Unit) {
    Row(Modifier.fillMaxWidth().padding(vertical = 2.dp).clip(RoundedCornerShape(11.dp))
        .background(if (selected) Accent.copy(alpha = 0.14f) else Color.Transparent)
        .clickable(onClick = onClick).padding(horizontal = 14.dp, vertical = 12.dp),
        verticalAlignment = Alignment.CenterVertically) {
        Text(label, color = if (selected) Color.White else Muted, fontSize = 14.sp, fontWeight = FontWeight.Medium)
    }
}

@Composable
private fun HomeContent(onStream: (String?) -> Unit) {
    val console = ConsoleStore.console
    Column(Modifier.fillMaxSize()) {
        Text("Home", color = Color.White, fontSize = 24.sp, fontWeight = FontWeight.SemiBold)
        Spacer(Modifier.height(18.dp))
        Row(Modifier.fillMaxWidth().clip(RoundedCornerShape(18.dp))
            .background(Brush.horizontalGradient(listOf(Accent.copy(alpha = 0.16f), Panel.copy(alpha = 0.4f)))).padding(18.dp),
            verticalAlignment = Alignment.CenterVertically) {
            Box(Modifier.size(58.dp).clip(RoundedCornerShape(15.dp)).background(Bg0).padding(12.dp), contentAlignment = Alignment.Center) {
                LogoMark(Modifier.fillMaxSize(), Accent)
            }
            Spacer(Modifier.width(14.dp))
            Column(Modifier.weight(1f)) {
                Text("PlayStation 5", color = OnDark, fontSize = 17.sp, fontWeight = FontWeight.SemiBold)
                Text(if (console != null) "${console.host} · Ready to stream" else "Tap Play to pair your PS5", color = Muted, fontSize = 12.sp)
            }
            Button(onClick = { onStream(null) }, colors = ButtonDefaults.buttonColors(containerColor = Accent),
                shape = RoundedCornerShape(14.dp), modifier = Modifier.height(54.dp).width(150.dp)) {
                Text(if (console != null) "Play" else "Pair", color = Color.White, fontSize = 18.sp, fontWeight = FontWeight.SemiBold)
            }
        }
        Spacer(Modifier.height(24.dp))
        Text("Continue playing", color = Color(0xFFCDD8EA), fontSize = 14.sp, fontWeight = FontWeight.SemiBold)
        Spacer(Modifier.height(12.dp))
        val covers = listOf(Color(0xFF6A1020), Color(0xFFB81E2E), Color(0xFF1F4F7D), Color(0xFF5A3A1F))
        LazyRow(horizontalArrangement = Arrangement.spacedBy(14.dp)) {
            items(covers) { c -> Box(Modifier.width(120.dp).height(156.dp).clip(RoundedCornerShape(13.dp)).background(Brush.verticalGradient(listOf(c, Bg0)))) }
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
private fun SettingsContent() {
    var quality by remember { mutableStateOf(StreamPrefs.quality) }
    var bitrate by remember { mutableStateOf(StreamPrefs.bitrateLabel()) }
    var controls by remember { mutableStateOf(StreamPrefs.controlsLabel()) }
    Column(Modifier.fillMaxSize().verticalScroll(rememberScrollState())) {
        Text("Settings", color = Color.White, fontSize = 24.sp, fontWeight = FontWeight.SemiBold)
        Text("Tune how PulsePlay streams from your PS5.", color = Muted, fontSize = 13.sp, modifier = Modifier.padding(top = 4.dp, bottom = 18.dp))
        Column(Modifier.fillMaxWidth().clip(RoundedCornerShape(16.dp)).background(Color.White.copy(alpha = 0.03f))) {
            SettingRow("Video quality", quality.summary, onClick = {
                quality = QualityPreset.values()[(quality.ordinal + 1) % QualityPreset.values().size]
                StreamPrefs.set(quality)
            })
            SettingRow("Bandwidth limit", bitrate, onClick = { StreamPrefs.cycleBitrate(); bitrate = StreamPrefs.bitrateLabel() })
            SettingRow("Controller & touch", controls, onClick = { StreamPrefs.toggleControls(); controls = StreamPrefs.controlsLabel() })
            SettingRow("Audio", "On")
            SettingRow("Microphone", "Coming soon", soon = true)
            AwaySetting()
        }
        Spacer(Modifier.height(16.dp))
        TextButton(onClick = { ConsoleStore.forget(); ConsoleStore.forgetLink() }) { Text("Forget this PS5", color = Color(0xFFF85149), fontSize = 14.sp) }
    }
}

/** "Play away from home": set up once on home Wi-Fi, then the Play button works anywhere. */
@Composable
private fun AwaySetting() {
    val scope = rememberCoroutineScope()
    val appCtx = androidx.compose.ui.platform.LocalContext.current.applicationContext
    var configured by remember { mutableStateOf(AwayManager.configured) }
    var busy by remember { mutableStateOf(false) }
    var note by remember { mutableStateOf<String?>(null) }
    var noteOk by remember { mutableStateOf(true) }

    val value = when {
        busy -> "Working…"
        configured -> "On"
        ConsoleStore.console == null -> "Pair first"
        else -> "Set up"
    }
    Column {
        SettingRow("Play away from home", value, soon = busy, onClick = {
            val console = ConsoleStore.console
            when {
                console == null -> { note = "Pair your PS5 on home Wi-Fi first."; noteOk = false }
                configured -> {
                    // Turn it off: ask the PS5 to leave (best effort), then drop the saved key.
                    busy = true; note = null
                    scope.launch {
                        withContext(Dispatchers.IO) {
                            runCatching { HelperClient.link("POST", console.host, console.token, action = "leave") }
                        }
                        ConsoleStore.forgetLink(); AwayManager.disconnect()
                        configured = false; busy = false; note = "Play away from home is off."; noteOk = true
                    }
                }
                else -> {
                    busy = true; note = null
                    scope.launch {
                        try {
                            AwayManager.setupAtHome(appCtx) { s -> note = s; noteOk = true }
                            configured = true
                        } catch (e: HelperError) { note = e.message; noteOk = false }
                        catch (e: Exception) { note = e.message ?: "Setup failed."; noteOk = false }
                        finally { busy = false }
                    }
                }
            }
        })
        note?.let {
            Text(it, color = if (noteOk) Good else Color(0xFFF85149), fontSize = 12.sp,
                modifier = Modifier.padding(start = 18.dp, end = 18.dp, bottom = 14.dp))
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

    Column(Modifier.fillMaxSize().padding(28.dp), horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.Center) {
        Text("Pair with your PS5", color = Color.White, fontSize = 24.sp, fontWeight = FontWeight.Bold)
        Text("Run the Orbit Play helper on your PS5, then enter its local IP and the 6-digit code it shows.",
            color = Muted, fontSize = 13.sp, modifier = Modifier.padding(top = 8.dp, bottom = 22.dp))
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
                    val reg = Registration.register(host, token)
                    ok = true; status = "Registered! ${reg.host} is ready. Tap Play on Home."
                } catch (e: HelperError) { ok = false; status = e.message ?: "Pairing failed." }
                catch (e: Exception) { ok = false; status = e.message ?: "Pairing failed." }
                finally { busy = false }
            }
        }, colors = ButtonDefaults.buttonColors(containerColor = Accent), shape = RoundedCornerShape(14.dp),
            modifier = Modifier.fillMaxWidth().height(54.dp).padding(top = 20.dp)) {
            Text(if (busy) "Connecting…" else "Connect", color = Color.White, fontSize = 17.sp)
        }
        if (status.isNotEmpty()) Text(status, color = if (ok) Good else Color(0xFFF85149), fontSize = 14.sp, modifier = Modifier.padding(top = 16.dp))
        TextButton(onClick = onBack, modifier = Modifier.padding(top = 8.dp)) { Text("Back", color = Muted, fontSize = 14.sp) }
    }
}

@Composable
private fun StreamScreen(onExit: () -> Unit) {
    val console = ConsoleStore.console
    if (console == null) { onExit(); return }
    val activity = androidx.compose.ui.platform.LocalContext.current as? android.app.Activity
    var status by remember { mutableStateOf("Starting…") }
    var quit by remember { mutableStateOf(false) }
    val appCtx = androidx.compose.ui.platform.LocalContext.current.applicationContext
    val session = remember { StreamSession(appCtx, console, { status = it }, { reason -> status = reason; quit = true }) }
    // When the stream ends (or drops), show the reason briefly then return Home — no hang.
    LaunchedEffect(quit) { if (quit) { delay(900); onExit() } }
    DisposableEffect(Unit) {
        // Keep the screen awake and lock to landscape while streaming.
        activity?.window?.addFlags(android.view.WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
        val prevOrientation = activity?.requestedOrientation
        activity?.requestedOrientation = android.content.pm.ActivityInfo.SCREEN_ORIENTATION_SENSOR_LANDSCAPE
        ActiveStream.session = session
        session.start()
        onDispose {
            session.stop(); ActiveStream.clear()
            activity?.window?.clearFlags(android.view.WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
            prevOrientation?.let { activity.requestedOrientation = it }
        }
    }
    Box(Modifier.fillMaxSize().background(Color.Black)) {
        AndroidView(factory = { ctx ->
            SurfaceView(ctx).apply {
                holder.addCallback(object : SurfaceHolder.Callback {
                    override fun surfaceCreated(h: SurfaceHolder) { session.setSurface(h.surface) }
                    override fun surfaceChanged(h: SurfaceHolder, f: Int, w: Int, ht: Int) {}
                    override fun surfaceDestroyed(h: SurfaceHolder) { session.setSurface(null) }
                })
            }
        }, modifier = Modifier.fillMaxSize())

        // On-screen controls, hidden live once a physical controller is in use (unless forced on).
        if (StreamPrefs.alwaysShowControls || !ActiveStream.hasGamepadState.value) GamepadOverlay()

        Text(status, color = Color.White, fontSize = 13.sp, modifier = Modifier.align(Alignment.TopStart).padding(16.dp))
        Button(onClick = onExit, colors = ButtonDefaults.buttonColors(containerColor = Color(0x66000000)),
            shape = RoundedCornerShape(12.dp), modifier = Modifier.align(Alignment.TopEnd).padding(16.dp)) {
            Text("Disconnect", color = Color.White, fontSize = 14.sp)
        }
    }
}
