package ai.storyteller.wordcraft

import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import android.content.Intent
import android.net.Uri
import android.os.Build
import android.os.Bundle
import android.provider.OpenableColumns
import android.util.Log
import android.view.View
import android.view.WindowManager
import androidx.activity.result.contract.ActivityResultContracts
import androidx.core.view.WindowCompat
import androidx.core.view.WindowInsetsCompat
import androidx.core.view.WindowInsetsControllerCompat
import com.google.androidgamesdk.GameActivity

/**
 * GameActivity host for the Rust `libwordcraft.so` (eframe / egui desktop UI).
 *
 * Responsibilities:
 * - Immersive edge-to-edge fullscreen (hides status bar + nav bar, swipe-to-reveal).
 * - Soft-keyboard / IME: shown/hidden in response to Rust requests via [showKeyboard] /
 *   [hideKeyboard]; the GameActivity InputConnection routes typed characters to wgpu/egui.
 * - File I/O via Android Storage Access Framework (SAF): [requestOpen] opens the system
 *   file picker; bytes are relayed to Rust via [nativeOnFileOpened]. [requestSave] writes
 *   bytes via a new SAF document.
 * - Share-to: incoming documents/text from other apps are relayed on [onStart].
 * - Back-gesture on API ≥ 33: handled natively by the predictive-back system.
 */
class MainActivity : GameActivity() {

    // ──────────────────────────── state ────────────────────────────────────────

    private var pendingSaveBytes: ByteArray? = null
    private var openPurpose: String = "document"

    // ────────────────────────── SAF launchers ──────────────────────────────────

    private val openDocument = registerForActivityResult(
        ActivityResultContracts.OpenDocument(),
    ) { uri: Uri? ->
        uri ?: return@registerForActivityResult
        // Persist read permission so the user doesn't have to pick the file again.
        try {
            contentResolver.takePersistableUriPermission(uri, Intent.FLAG_GRANT_READ_URI_PERMISSION)
        } catch (_: SecurityException) { /* not all providers support this */ }
        relayFileToNative(uri)
    }

    private val createDocument = registerForActivityResult(
        ActivityResultContracts.CreateDocument("*/*"),
    ) { uri: Uri? ->
        val bytes = pendingSaveBytes.also { pendingSaveBytes = null } ?: return@registerForActivityResult
        if (uri == null) return@registerForActivityResult
        try {
            contentResolver.openOutputStream(uri)?.use { it.write(bytes) }
                ?: Log.e(TAG, "no output stream for $uri")
        } catch (e: Exception) {
            Log.e(TAG, "save failed", e)
        }
    }

    // ──────────────────────── lifecycle ────────────────────────────────────────

    override fun onCreate(savedInstanceState: Bundle?) {
        instance = this
        super.onCreate(savedInstanceState)
        applyImmersiveFullscreen()
        handleIncomingIntent(intent)
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        setIntent(intent)
        handleIncomingIntent(intent)
    }

    override fun onStart() {
        super.onStart()
        // Re-apply immersive mode after returning from another activity (e.g. file picker).
        applyImmersiveFullscreen()
    }

    override fun onWindowFocusChanged(hasFocus: Boolean) {
        super.onWindowFocusChanged(hasFocus)
        if (hasFocus) applyImmersiveFullscreen()
    }

    override fun onDestroy() {
        if (instance === this) instance = null
        super.onDestroy()
    }

    // ─────────────────────── fullscreen / insets ───────────────────────────────

    private fun applyImmersiveFullscreen() {
        window.addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
        WindowCompat.setDecorFitsSystemWindows(window, false)
        val ctrl = WindowCompat.getInsetsController(window, window.decorView)
        ctrl.systemBarsBehavior = WindowInsetsControllerCompat.BEHAVIOR_SHOW_TRANSIENT_BARS_BY_SWIPE
        ctrl.hide(WindowInsetsCompat.Type.systemBars())

        // On API ≥ 28 allow content to draw into display cut-outs.
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.P) {
            window.attributes = window.attributes.also { attrs ->
                attrs.layoutInDisplayCutoutMode =
                    WindowManager.LayoutParams.LAYOUT_IN_DISPLAY_CUTOUT_MODE_SHORT_EDGES
            }
        }
    }

    // ─────────────────────── intent handling ───────────────────────────────────

    /** VIEW / EDIT / SEND intent when the app is launched from another app. */
    private fun handleIncomingIntent(intent: Intent?) {
        intent ?: return
        when (intent.action) {
            Intent.ACTION_VIEW, Intent.ACTION_EDIT -> {
                intent.data?.let { relayFileToNative(it) }
            }
            Intent.ACTION_SEND -> {
                // Text shared from another app → insert as new document content.
                val text = intent.getStringExtra(Intent.EXTRA_TEXT)
                if (text != null) {
                    nativePushText(text)
                    return
                }
                // Document file shared to WordCraft.
                val uri = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
                    intent.getParcelableExtra(Intent.EXTRA_STREAM, Uri::class.java)
                } else {
                    @Suppress("DEPRECATION")
                    intent.getParcelableExtra(Intent.EXTRA_STREAM)
                }
                uri?.let { relayFileToNative(it) }
            }
        }
    }

    // ─────────────────────── file relay helpers ─────────────────────────────────

    private fun relayFileToNative(uri: Uri) {
        val name = displayName(uri) ?: uri.lastPathSegment ?: "document"
        val bytes = try {
            contentResolver.openInputStream(uri)?.use { it.readBytes() }
        } catch (e: Exception) {
            Log.e(TAG, "cannot read $uri", e)
            null
        }
        if (bytes == null) {
            Log.e(TAG, "failed to read $uri")
            return
        }
        try {
            nativeOnFileOpened(name, bytes)
        } catch (e: UnsatisfiedLinkError) {
            Log.e(TAG, "nativeOnFileOpened not linked", e)
        }
    }

    private fun displayName(uri: Uri): String? {
        contentResolver.query(uri, arrayOf(OpenableColumns.DISPLAY_NAME), null, null, null)
            ?.use { cursor ->
                if (cursor.moveToFirst()) {
                    val idx = cursor.getColumnIndex(OpenableColumns.DISPLAY_NAME)
                    if (idx >= 0) return cursor.getString(idx)
                }
            }
        return null
    }

    // ─────────────────────── keyboard (IME) ─────────────────────────────────────

    /**
     * Called from Rust/JNI to show the soft keyboard.
     * Uses GameActivity's native showIme() so GameTextInput's InputConnection receives
     * commitText, setComposingText and character inputs.
     */
    fun showKeyboard() {
        showIme(0)
    }

    /** Called from Rust/JNI to dismiss the soft keyboard. */
    fun hideKeyboard() {
        hideIme(0)
    }

    // ─────────────────────── open / save launchers ──────────────────────────────

    private fun launchOpen(purpose: String) {
        openPurpose = purpose
        val mimes = if (purpose == "picture") {
            arrayOf("image/*")
        } else {
            arrayOf(
                "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
                "application/msword",
                "application/vnd.oasis.opendocument.text",
                "application/rtf",
                "text/plain",
                "text/html",
                "text/markdown",
                "application/json",
                "*/*",
            )
        }
        openDocument.launch(mimes)
    }

    private fun launchSave(name: String, bytes: ByteArray) {
        pendingSaveBytes = bytes
        createDocument.launch(name)
    }

    // ─────────────────────── companion / JNI bridge ─────────────────────────────

    companion object {
        private const val TAG = "WordCraft"

        @Volatile
        private var instance: MainActivity? = null

        // ── called from Rust via JNI ───────────────────────────────────────────

        @JvmStatic
        fun requestOpen(purpose: String) {
            val act = instance ?: run { Log.w(TAG, "requestOpen: activity not ready"); return }
            act.runOnUiThread { act.launchOpen(purpose) }
        }

        @JvmStatic
        fun requestSave(name: String, bytes: ByteArray) {
            val act = instance ?: run { Log.w(TAG, "requestSave: activity not ready"); return }
            act.runOnUiThread { act.launchSave(name, bytes) }
        }

        /** Show the IME / soft keyboard — called from Rust when egui IME output is set. */
        @JvmStatic
        fun requestShowKeyboard() {
            val act = instance ?: return
            act.runOnUiThread { act.showKeyboard() }
        }

        /** Hide the IME / soft keyboard — called from Rust when egui IME output is cleared. */
        @JvmStatic
        fun requestHideKeyboard() {
            val act = instance ?: return
            act.runOnUiThread { act.hideKeyboard() }
        }

        // ── called from Kotlin, implemented in Rust ───────────────────────────

        /** Deliver a file's bytes to the Rust engine. */
        @JvmStatic
        private external fun nativeOnFileOpened(name: String, bytes: ByteArray)

        /** Deliver plain text shared from another app. */
        @JvmStatic
        private external fun nativePushText(text: String)
    }
}
