/*
 * PulsePlay — free, open-source Remote Play for jailbroken PS5s (no PSN).
 * Copyright (C) 2026 PulsePlay contributors.
 *
 * This program is free software: you can redistribute it and/or modify it under
 * the terms of the GNU Affero General Public License as published by the Free
 * Software Foundation, either version 3 of the License, or (at your option) any
 * later version. See <https://www.gnu.org/licenses/>.
 */
package dev.pulseplay

import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.darkColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.tooling.preview.Preview
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp

// Dark, minimal palette with an accent blue — the M4 visual direction, started early.
private val Background = Color(0xFF0E1116)
private val Surface = Color(0xFF161B22)
private val Accent = Color(0xFF3B82F6)
private val OnDark = Color(0xFFE6EDF3)
private val Muted = Color(0xFF8B949E)

private val PulsePlayColors = darkColorScheme(
    primary = Accent,
    background = Background,
    surface = Surface,
    onBackground = OnDark,
    onSurface = OnDark,
)

class MainActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()
        setContent {
            MaterialTheme(colorScheme = PulsePlayColors) {
                Surface(modifier = Modifier.fillMaxSize(), color = Background) {
                    HomeScreen()
                }
            }
        }
    }
}

@Composable
private fun HomeScreen() {
    Column(
        modifier = Modifier
            .fillMaxSize()
            .padding(24.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.Center,
    ) {
        Text("PulsePlay", color = OnDark, fontSize = 34.sp, fontWeight = FontWeight.Bold)
        Text(
            "Remote Play for jailbroken PS5 — no PSN",
            color = Muted,
            fontSize = 14.sp,
            modifier = Modifier.padding(top = 6.dp, bottom = 40.dp),
        )

        // The big Play card: the single primary action the home screen is built around.
        Button(
            onClick = { /* M1: wire up discovery + stream next */ },
            colors = ButtonDefaults.buttonColors(containerColor = Accent),
            shape = RoundedCornerShape(20.dp),
            modifier = Modifier
                .fillMaxWidth()
                .height(120.dp),
        ) {
            Text("Play", color = Color.White, fontSize = 28.sp, fontWeight = FontWeight.SemiBold)
        }

        Text(
            "No PS5 added yet",
            color = Muted,
            fontSize = 13.sp,
            modifier = Modifier.padding(top = 20.dp),
        )
    }
}

@Preview
@Composable
private fun HomePreview() {
    MaterialTheme(colorScheme = PulsePlayColors) {
        Box(Modifier.fillMaxSize()) { HomeScreen() }
    }
}
