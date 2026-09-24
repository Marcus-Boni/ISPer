package com.isper.mobile

import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.ui.graphics.Color

// As mesmas cores do desktop (apps/isper-app/ui/assets/base.css): escuro
// quente com terracota rara, e o tema claro já auditado para contraste AA.
private val Dark = darkColorScheme(
    primary = Color(0xFFF07E72),
    onPrimary = Color(0xFF1B0F0D),
    secondary = Color(0xFF84C297),
    onSecondary = Color(0xFF10170F),
    tertiary = Color(0xFF7CC4F0),
    background = Color(0xFF161311),
    onBackground = Color(0xFFECE7E1),
    surface = Color(0xFF1E1A18),
    onSurface = Color(0xFFECE7E1),
    surfaceVariant = Color(0xFF26211E),
    onSurfaceVariant = Color(0xFFA79E96),
    surfaceContainer = Color(0xFF1E1A18),
    surfaceContainerHigh = Color(0xFF26211E),
    outline = Color(0xFF4A403A),
    outlineVariant = Color(0xFF372F2B),
    error = Color(0xFFF79F94),
)

private val Light = lightColorScheme(
    primary = Color(0xFFB03F36),
    onPrimary = Color(0xFFFFFFFF),
    secondary = Color(0xFF2B6F42),
    onSecondary = Color(0xFFFFFFFF),
    tertiary = Color(0xFF1B6597),
    background = Color(0xFFF6F1EC),
    onBackground = Color(0xFF1F1915),
    surface = Color(0xFFFFFDFB),
    onSurface = Color(0xFF1F1915),
    surfaceVariant = Color(0xFFF3ECE5),
    onSurfaceVariant = Color(0xFF655950),
    surfaceContainer = Color(0xFFFFFDFB),
    surfaceContainerHigh = Color(0xFFF3ECE5),
    outline = Color(0xFFCDBFB2),
    outlineVariant = Color(0xFFE0D5CA),
    error = Color(0xFF93322A),
)

@Composable
fun IsperTheme(content: @Composable () -> Unit) {
    MaterialTheme(colorScheme = if (isSystemInDarkTheme()) Dark else Light, content = content)
}
