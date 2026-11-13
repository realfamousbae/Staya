package io.github.realfamousbae.staya

import android.content.Intent
import android.graphics.Color
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.SystemBarStyle
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.compose.foundation.background
import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.safeDrawingPadding
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.dp
import io.github.realfamousbae.staya.location.LocationShare
import io.github.realfamousbae.staya.ui.AppModel
import io.github.realfamousbae.staya.ui.DeepLink
import io.github.realfamousbae.staya.ui.FriendsModel
import io.github.realfamousbae.staya.ui.FriendsScreen
import io.github.realfamousbae.staya.ui.Ink
import io.github.realfamousbae.staya.ui.OnboardingScreen
import io.github.realfamousbae.staya.ui.Pill
import io.github.realfamousbae.staya.ui.StayaTheme

class MainActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setBars(lightIcons = true)
        if (savedInstanceState == null) handleLink(intent)
        setContent {
            StayaTheme {
                Root { lightIcons -> setBars(lightIcons) }
            }
        }
    }

    /** Системные панели прозрачные; светлые значки на тёмных экранах, тёмные — над светлой картой. */
    private fun setBars(lightIcons: Boolean) {
        val style = if (lightIcons) SystemBarStyle.dark(Color.TRANSPARENT) else SystemBarStyle.light(Color.TRANSPARENT, Color.TRANSPARENT)
        enableEdgeToEdge(statusBarStyle = style, navigationBarStyle = style)
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        handleLink(intent)
    }

    /** Ссылка staya://… из другого приложения — только в поле онбординга или на подтверждение. */
    private fun handleLink(intent: Intent?) {
        DeepLink.parse(intent?.dataString)?.let { AppModel.pendingLink = it }
    }

    override fun onResume() {
        super.onResume()
        AppCore.openAsync(this)
        AppModel.foreground = true
        // Сервис мог быть остановлен системой: поднять снова, если передача включена.
        LocationShare.startService(this, fromBackground = false)
    }

    override fun onPause() {
        AppModel.foreground = false
        FriendsModel.stop()
        super.onPause()
    }
}

/** Корень: ядро открывается лениво, затем онбординг или главный экран (4.2). */
@Composable
private fun Root(onBars: (lightIcons: Boolean) -> Unit) {
    val state = AppCore.state
    val home = state is AppCore.State.Open && AppModel.phase is AppModel.Phase.Ready && FriendsModel.screen == FriendsModel.Screen.Home
    val darkMap = isSystemInDarkTheme()
    // Над светлой картой — тёмные значки панелей; везде остальное тёмное.
    LaunchedEffect(home, darkMap) { onBars(!home || darkMap) }

    Box(modifier = Modifier.fillMaxSize().background(Ink.ink)) {
        when (state) {
            is AppCore.State.Open -> {
                LaunchedEffect(state) { AppModel.refresh(state.core, state.accountId) }
                when (val phase = AppModel.phase) {
                    AppModel.Phase.Loading -> Spinner()
                    AppModel.Phase.Onboarding -> OnboardingScreen(state.core, state.accountId)
                    is AppModel.Phase.Ready -> {
                        // На экране — синхронизация и WebSocket; при уходе в фон — стоп (onPause).
                        val context = LocalContext.current
                        LaunchedEffect(state, AppModel.foreground) {
                            if (AppModel.foreground) FriendsModel.start(state.core, context)
                        }
                        LaunchedEffect(AppModel.pendingLink) {
                            AppModel.pendingLink?.let {
                                AppModel.pendingLink = null
                                FriendsModel.open(it)
                            }
                        }
                        FriendsScreen(state.core, phase.nick, phase.server)
                    }
                }
            }
            AppCore.State.Closed, AppCore.State.Opening -> Spinner()
            is AppCore.State.Unavailable -> Column(modifier = Modifier.safeDrawingPadding().padding(20.dp)) {
                Text("Не удалось открыть данные: ${state.reason}", color = Ink.paper)
            }
            is AppCore.State.Broken -> Column(modifier = Modifier.safeDrawingPadding().padding(20.dp)) {
                Text("Данные повреждены: ${state.reason}", color = Ink.paper, modifier = Modifier.padding(bottom = 16.dp))
                val context = LocalContext.current
                Pill("Сбросить локальные данные", { AppCore.resetAsync(context) })
            }
        }
    }
}

@Composable
private fun Spinner() {
    Box(modifier = Modifier.fillMaxSize(), contentAlignment = Alignment.Center) {
        CircularProgressIndicator(color = Ink.mint)
    }
}
