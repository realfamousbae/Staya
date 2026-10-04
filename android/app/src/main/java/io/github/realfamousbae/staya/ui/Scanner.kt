package io.github.realfamousbae.staya.ui

import androidx.camera.core.CameraSelector
import androidx.camera.core.ImageAnalysis
import androidx.camera.core.Preview
import androidx.camera.lifecycle.ProcessCameraProvider
import androidx.camera.view.PreviewView
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.remember
import androidx.compose.ui.Modifier
import androidx.compose.ui.viewinterop.AndroidView
import androidx.core.content.ContextCompat
import androidx.lifecycle.compose.LocalLifecycleOwner
import com.google.zxing.PlanarYUVLuminanceSource
import java.util.concurrent.Executors
import java.util.concurrent.atomic.AtomicBoolean

/**
 * Сканер QR-кода приглашения: CameraX + ZXing по яркостной плоскости кадра.
 * Находит `staya://…` один раз и отдаёт в [onResult]; кадры нигде не сохраняются.
 */
@Composable
fun QrScanner(modifier: Modifier, onResult: (DeepLink) -> Unit) {
    val owner = LocalLifecycleOwner.current
    val analysis = remember { Executors.newSingleThreadExecutor() }
    val done = remember { AtomicBoolean(false) }
    val provider = remember { java.util.concurrent.atomic.AtomicReference<ProcessCameraProvider?>() }

    AndroidView(
        modifier = modifier,
        factory = { ctx ->
            val view = PreviewView(ctx)
            val future = ProcessCameraProvider.getInstance(ctx)
            future.addListener({
                val p = future.get().also { provider.set(it) }
                val preview = Preview.Builder().build().also { it.surfaceProvider = view.surfaceProvider }
                val analyzer = ImageAnalysis.Builder()
                    .setBackpressureStrategy(ImageAnalysis.STRATEGY_KEEP_ONLY_LATEST)
                    .build()
                analyzer.setAnalyzer(analysis) { image ->
                    image.use {
                        if (done.get()) return@use
                        val plane = it.planes[0]
                        val bytes = ByteArray(plane.buffer.remaining()).also { b -> plane.buffer.get(b) }
                        val source = PlanarYUVLuminanceSource(
                            bytes, plane.rowStride, it.height, 0, 0, it.width, it.height, false,
                        )
                        val link = DeepLink.parse(Qr.decode(source))
                        if (link != null && done.compareAndSet(false, true)) {
                            ContextCompat.getMainExecutor(ctx).execute { onResult(link) }
                        }
                    }
                }
                p.unbindAll()
                p.bindToLifecycle(owner, CameraSelector.DEFAULT_BACK_CAMERA, preview, analyzer)
            }, ContextCompat.getMainExecutor(ctx))
            view
        },
    )
    DisposableEffect(Unit) {
        onDispose {
            provider.get()?.unbindAll()
            analysis.shutdown()
        }
    }
}
