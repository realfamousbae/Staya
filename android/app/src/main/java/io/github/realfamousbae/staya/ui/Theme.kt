package io.github.realfamousbae.staya.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.RowScope
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.safeDrawingPadding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.Typography
import androidx.compose.material3.darkColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.shadow
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import io.github.realfamousbae.staya.R

/**
 * Оформление (задача 5.2): карта во весь экран, тёмный хром поверх неё, тёмные
 * экраны с белыми карточками — на манер Blink, без лишнего. Одна смелость —
 * крупный строчный заголовок-состояние; всё остальное тихо.
 */
object Ink {
    val ink = Color(0xFF0F0F12)
    val ink2 = Color(0xFF1C1C22)
    val ink3 = Color(0xFF2A2A33)
    val paper = Color(0xFFFFFFFF)
    val mist = Color(0xFF9A9AA6)
    val mint = Color(0xFFA9F0C7)
    val sky = Color(0xFF9BD0FF)
    val fresh = Color(0xFF4CD97B)
    val danger = Color(0xFFFF6B6B)
    val accent = Brush.horizontalGradient(listOf(mint, sky))
}

private val scheme = darkColorScheme(
    primary = Ink.mint,
    onPrimary = Ink.ink,
    secondary = Ink.sky,
    onSecondary = Ink.ink,
    background = Ink.ink,
    onBackground = Ink.paper,
    surface = Ink.ink,
    onSurface = Ink.paper,
    surfaceVariant = Ink.ink2,
    onSurfaceVariant = Ink.mist,
    surfaceContainer = Ink.ink2,
    surfaceContainerHigh = Ink.ink2,
    surfaceContainerHighest = Ink.ink3,
    outline = Ink.ink3,
    outlineVariant = Ink.ink3,
    error = Ink.danger,
    onError = Ink.ink,
)

private val type = Typography(
    // Заголовок экрана и состояние на карте: крупный, жирный, строчные буквы.
    displaySmall = TextStyle(fontSize = 34.sp, lineHeight = 38.sp, fontWeight = FontWeight.ExtraBold, letterSpacing = (-0.5).sp),
    titleLarge = TextStyle(fontSize = 24.sp, lineHeight = 28.sp, fontWeight = FontWeight.Bold, letterSpacing = (-0.3).sp),
    titleMedium = TextStyle(fontSize = 17.sp, lineHeight = 22.sp, fontWeight = FontWeight.SemiBold),
    bodyLarge = TextStyle(fontSize = 16.sp, lineHeight = 22.sp),
    bodyMedium = TextStyle(fontSize = 15.sp, lineHeight = 21.sp),
    bodySmall = TextStyle(fontSize = 13.sp, lineHeight = 18.sp),
    labelLarge = TextStyle(fontSize = 16.sp, lineHeight = 20.sp, fontWeight = FontWeight.SemiBold),
)

@Composable
fun StayaTheme(content: @Composable () -> Unit) {
    MaterialTheme(colorScheme = scheme, typography = type, content = content)
}

/** Круглая чёрная кнопка поверх карты. */
@Composable
fun RoundButton(icon: Int, description: String, onClick: () -> Unit, modifier: Modifier = Modifier, size: Int = 56) {
    Box(
        modifier = modifier
            .size(size.dp)
            .shadow(8.dp, CircleShape, ambientColor = Color.Black.copy(alpha = 0.4f), spotColor = Color.Black.copy(alpha = 0.4f))
            .clip(CircleShape)
            .background(Ink.ink)
            .clickable(role = Role.Button, onClick = onClick),
        contentAlignment = Alignment.Center,
    ) {
        Icon(painterResource(icon), contentDescription = description, tint = Ink.paper, modifier = Modifier.size((size * 0.45).dp))
    }
}

/** Главное действие экрана: пилюля с градиентом. */
@Composable
fun Pill(text: String, onClick: () -> Unit, modifier: Modifier = Modifier, enabled: Boolean = true) {
    Box(
        modifier = modifier
            .fillMaxWidth()
            .height(52.dp)
            .clip(RoundedCornerShape(26.dp))
            .background(if (enabled) Ink.accent else Brush.horizontalGradient(listOf(Ink.ink3, Ink.ink3)))
            .clickable(enabled = enabled, role = Role.Button, onClick = onClick),
        contentAlignment = Alignment.Center,
    ) {
        Text(text, style = MaterialTheme.typography.labelLarge, color = if (enabled) Ink.ink else Ink.mist)
    }
}

/** Второстепенное действие: тихая пилюля на тёмном. */
@Composable
fun SoftButton(onClick: () -> Unit, modifier: Modifier = Modifier, enabled: Boolean = true, content: @Composable RowScope.() -> Unit) {
    Row(
        modifier = modifier
            .fillMaxWidth()
            .height(48.dp)
            .clip(RoundedCornerShape(24.dp))
            .background(Ink.ink2)
            .clickable(enabled = enabled, role = Role.Button, onClick = onClick)
            .padding(horizontal = 20.dp),
        horizontalArrangement = Arrangement.Center,
        verticalAlignment = Alignment.CenterVertically,
        content = content,
    )
}

/** Строка списка: заголовок, подпись, стрелка; разделяет только отступ. */
@Composable
fun ListRow(
    title: String,
    subtitle: String? = null,
    subtitleColor: Color = Ink.mist,
    leading: (@Composable () -> Unit)? = null,
    trailing: (@Composable () -> Unit)? = null,
    enabled: Boolean = true,
    onClick: (() -> Unit)? = null,
) {
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .clip(RoundedCornerShape(16.dp))
            .then(if (onClick != null) Modifier.clickable(enabled = enabled, onClick = onClick) else Modifier)
            .padding(horizontal = 4.dp, vertical = 10.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        if (leading != null) {
            leading()
            Spacer(Modifier.width(14.dp))
        }
        Column(modifier = Modifier.weight(1f)) {
            Text(title, style = MaterialTheme.typography.titleMedium, color = if (enabled) Ink.paper else Ink.mist)
            if (subtitle != null) Text(subtitle, style = MaterialTheme.typography.bodySmall, color = subtitleColor)
        }
        if (trailing != null) {
            trailing()
        } else if (onClick != null) {
            Icon(painterResource(R.drawable.ic_chevron), contentDescription = null, tint = Ink.mist, modifier = Modifier.size(22.dp))
        }
    }
}

/** Заголовок раздела на тёмном экране. */
@Composable
fun SectionTitle(text: String) {
    Text(text, style = MaterialTheme.typography.titleLarge, color = Ink.paper, modifier = Modifier.padding(top = 12.dp))
}

/**
 * Тёмный экран с круглой кнопкой «назад» и крупным строчным заголовком; содержимое
 * прокручивается. Сам учитывает вырезы и системные панели.
 */
@Composable
fun DarkScreen(title: String, onBack: (() -> Unit)?, content: @Composable () -> Unit) {
    Column(
        modifier = Modifier
            .fillMaxSize()
            .background(Ink.ink)
            .safeDrawingPadding()
            .verticalScroll(rememberScrollState())
            .padding(horizontal = 20.dp)
            .padding(bottom = 24.dp),
        verticalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        Row(modifier = Modifier.padding(top = 12.dp), verticalAlignment = Alignment.CenterVertically) {
            if (onBack != null) {
                RoundButton(R.drawable.ic_back, "Назад", onBack, size = 44)
                Spacer(Modifier.width(14.dp))
            }
            Text(title, style = MaterialTheme.typography.displaySmall, color = Ink.paper)
        }
        content()
    }
}
