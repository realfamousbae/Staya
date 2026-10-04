package io.github.realfamousbae.staya

import android.content.Intent
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.safeDrawingPadding
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.dp
import io.github.realfamousbae.staya.ui.AppModel
import io.github.realfamousbae.staya.ui.DeepLink
import io.github.realfamousbae.staya.ui.FriendsModel
import io.github.realfamousbae.staya.ui.FriendsScreen
import io.github.realfamousbae.staya.ui.OnboardingScreen

class MainActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()
        if (savedInstanceState == null) handleLink(intent)
        setContent {
            MaterialTheme {
                Surface(modifier = Modifier.fillMaxSize()) {
                    Root()
                }
            }
        }
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
    }

    override fun onPause() {
        AppModel.foreground = false
        FriendsModel.stop()
        super.onPause()
    }
}

/** Корень: ядро открывается лениво, затем онбординг или главный экран (4.2). */
@Composable
private fun Root() {
    Column(modifier = Modifier.fillMaxSize().safeDrawingPadding()) {
        when (val state = AppCore.state) {
            is AppCore.State.Open -> {
                LaunchedEffect(state) { AppModel.refresh(state.core, state.accountId) }
                when (val phase = AppModel.phase) {
                    AppModel.Phase.Loading -> CircularProgressIndicator(modifier = Modifier.padding(16.dp))
                    AppModel.Phase.Onboarding -> OnboardingScreen(state.core, state.accountId)
                    is AppModel.Phase.Ready -> {
                        // На экране — синхронизация и WebSocket; при уходе в фон — стоп (onPause).
                        LaunchedEffect(state, AppModel.foreground) {
                            if (AppModel.foreground) FriendsModel.start(state.core)
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
            AppCore.State.Closed, AppCore.State.Opening -> CircularProgressIndicator(modifier = Modifier.padding(16.dp))
            is AppCore.State.Unavailable -> Text("Не удалось открыть данные: ${state.reason}", modifier = Modifier.padding(16.dp))
            is AppCore.State.Broken -> Column(modifier = Modifier.padding(16.dp)) {
                Text("Данные повреждены: ${state.reason}")
                val context = LocalContext.current
                OutlinedButton(onClick = { AppCore.resetAsync(context) }) { Text("Сбросить локальные данные") }
            }
        }
    }
}
