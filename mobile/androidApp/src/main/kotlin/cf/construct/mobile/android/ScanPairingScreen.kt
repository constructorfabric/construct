package cf.construct.mobile.android

import android.Manifest
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.net.Uri
import android.provider.Settings
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.camera.core.CameraSelector
import androidx.camera.core.ImageAnalysis
import androidx.camera.core.Preview
import androidx.camera.lifecycle.ProcessCameraProvider
import androidx.camera.view.PreviewView
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.dp
import androidx.compose.ui.viewinterop.AndroidView
import androidx.core.content.ContextCompat
import androidx.lifecycle.compose.LocalLifecycleOwner
import cf.construct.mobile.core.designsystem.Spacing
import cf.construct.mobile.feature.status.ComputerOperation
import java.util.concurrent.Executors
import java.util.concurrent.atomic.AtomicBoolean

@Composable
internal fun ScanPairingScreen(
    operation: ComputerOperation,
    pair: (String) -> Unit,
    retry: () -> Unit,
    allowNetwork: () -> Unit,
    openManual: () -> Unit,
) {
    val context = LocalContext.current
    var asked by rememberSaveable { mutableStateOf(false) }
    var granted by remember {
        mutableStateOf(
            ContextCompat.checkSelfPermission(context, Manifest.permission.CAMERA) == PackageManager.PERMISSION_GRANTED,
        )
    }
    var awaitingPermission by remember { mutableStateOf(false) }
    // After the second refusal the system answers at once with no dialog, so the text says where to turn it on.
    var denied by rememberSaveable { mutableStateOf(false) }
    var cameraUnavailable by remember { mutableStateOf(false) }
    val permission =
        rememberLauncherForActivityResult(ActivityResultContracts.RequestPermission()) {
            granted = it
            denied = !it
            awaitingPermission = false
        }
    val ask = {
        awaitingPermission = true
        permission.launch(Manifest.permission.CAMERA)
    }
    LaunchedEffect(Unit) {
        if (!asked && !granted) {
            asked = true
            ask()
        }
    }
    Column(
        Modifier.fillMaxSize().padding(Spacing.L.dp),
        verticalArrangement = Arrangement.spacedBy(Spacing.M.dp),
    ) {
        when {
            awaitingPermission -> {
                CircularProgressIndicator()
            }

            !granted -> {
                CameraPermissionNeeded(denied, ask) { openAppSettings(context) }
            }

            cameraUnavailable -> {
                Text("The camera is unavailable. You can enter the address and code instead.")
            }

            operation.busy || operation.paired -> {
                // Pause the camera while the current offer is being handled.
            }

            operation.message != null -> {
                if (!operation.needsNetworkPermission) {
                    TextButton(onClick = retry) { Text("Scan again") }
                }
            }

            else -> {
                Text("Point your camera at the pairing QR code on your computer.")
                PairingCamera(Modifier.weight(1f).fillMaxWidth(), pair) { cameraUnavailable = true }
            }
        }
        OperationFeedback(operation, allowNetwork)
        TextButton(onClick = openManual, enabled = !operation.busy) { Text("Enter address and code") }
    }
}

@Composable
private fun CameraPermissionNeeded(
    denied: Boolean,
    allow: () -> Unit,
    openSettings: () -> Unit,
) {
    if (denied) {
        Text(
            "Camera access is off for Construct. Turn it on in the phone's settings, " +
                "or enter the address and code instead.",
        )
        TextButton(onClick = openSettings) { Text("Open phone settings") }
    } else {
        Text("Allow camera access to scan the pairing code.")
    }
    TextButton(onClick = allow) { Text("Allow camera access") }
}

private fun openAppSettings(context: Context) {
    context.startActivity(
        Intent(Settings.ACTION_APPLICATION_DETAILS_SETTINGS, Uri.fromParts("package", context.packageName, null))
            .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK),
    )
}

@Composable
private fun PairingCamera(
    modifier: Modifier,
    onPayload: (String) -> Unit,
    onUnavailable: () -> Unit,
) {
    val context = LocalContext.current
    val owner = LocalLifecycleOwner.current
    val currentPayload by rememberUpdatedState(onPayload)
    val currentUnavailable by rememberUpdatedState(onUnavailable)
    val view = remember(context) { PreviewView(context).apply { scaleType = PreviewView.ScaleType.FIT_CENTER } }

    AndroidView(factory = { view }, modifier = modifier)
    DisposableEffect(view, owner) {
        val main = ContextCompat.getMainExecutor(context)
        val worker = Executors.newSingleThreadExecutor()
        val disposed = AtomicBoolean()
        val delivered = AtomicBoolean()
        val provider = ProcessCameraProvider.getInstance(context)
        val preview = Preview.Builder().build().apply { surfaceProvider = view.surfaceProvider }
        val analysis = ImageAnalysis.Builder().setBackpressureStrategy(ImageAnalysis.STRATEGY_KEEP_ONLY_LATEST).build()
        val decoder = PairingQrReader()
        analysis.setAnalyzer(worker) { image ->
            val payload =
                try {
                    val plane = image.planes.first()
                    val buffer = plane.buffer
                    val luma = ByteArray(buffer.remaining()).also(buffer::get)
                    decoder.read(luma, image.width, image.height, plane.rowStride)
                } finally {
                    image.close()
                }
            if (payload != null && delivered.compareAndSet(false, true)) {
                main.execute { if (!disposed.get()) currentPayload(payload) }
            }
        }
        provider.addListener(
            {
                if (!disposed.get()) {
                    runCatching {
                        provider.get().bindToLifecycle(owner, CameraSelector.DEFAULT_BACK_CAMERA, preview, analysis)
                    }.onFailure { currentUnavailable() }
                }
            },
            main,
        )
        onDispose {
            disposed.set(true)
            analysis.clearAnalyzer()
            if (provider.isDone) runCatching { provider.get().unbind(preview, analysis) }
            worker.shutdown()
        }
    }
}
