package ai.storyteller.wordcraft

import android.Manifest
import android.content.Intent
import android.content.pm.PackageManager
import android.net.Uri
import android.os.Build
import android.os.Bundle
import android.os.SystemClock
import android.provider.OpenableColumns
import android.text.InputType
import android.util.Log
import android.view.KeyEvent
import android.view.WindowManager
import android.view.inputmethod.EditorInfo
import android.view.inputmethod.InputMethodManager
import androidx.activity.result.contract.ActivityResultContracts
import androidx.core.view.WindowCompat
import androidx.core.view.WindowInsetsCompat
import androidx.core.view.WindowInsetsControllerCompat
import com.google.androidgamesdk.GameActivity
import com.google.androidgamesdk.gametextinput.State
import java.io.File

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
    private var currentDocumentUri: Uri? = null

    // ────────────────────────── SAF launchers ──────────────────────────────────

    private val openDocument = registerForActivityResult(
        ActivityResultContracts.OpenDocument(),
    ) { uri: Uri? ->
        uri ?: return@registerForActivityResult
        currentDocumentUri = uri
        val flags = Intent.FLAG_GRANT_READ_URI_PERMISSION or Intent.FLAG_GRANT_WRITE_URI_PERMISSION
        try {
            contentResolver.takePersistableUriPermission(uri, flags)
        } catch (e: Exception) {
            Log.w(TAG, "Could not persist URI permissions for $uri", e)
        }
        relayFileToNative(uri)
    }

    private val createDocument = registerForActivityResult(
        ActivityResultContracts.CreateDocument("*/*"),
    ) { uri: Uri? ->
        val bytes = pendingSaveBytes.also { pendingSaveBytes = null } ?: return@registerForActivityResult
        if (uri == null) return@registerForActivityResult
        currentDocumentUri = uri
        val flags = Intent.FLAG_GRANT_READ_URI_PERMISSION or Intent.FLAG_GRANT_WRITE_URI_PERMISSION
        try {
            contentResolver.takePersistableUriPermission(uri, flags)
        } catch (e: Exception) {
            Log.w(TAG, "Could not persist URI permissions for $uri", e)
        }
        writeBytesToUri(uri, bytes)
        if (bytes.isNotEmpty()) {
            saveInternalBackup(bytes)
        }
    }

    private val requestPermissionsLauncher = registerForActivityResult(
        ActivityResultContracts.RequestMultiplePermissions(),
    ) { permissions ->
        permissions.forEach { (permission, isGranted) ->
            if (!isGranted) {
                Log.w(TAG, "Permission denied: $permission")
            }
        }
    }

    private fun checkAndRequestPermissions() {
        val needed = mutableListOf<String>()
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
            if (checkSelfPermission(Manifest.permission.READ_MEDIA_IMAGES) != PackageManager.PERMISSION_GRANTED) {
                needed.add(Manifest.permission.READ_MEDIA_IMAGES)
            }
            if (checkSelfPermission(Manifest.permission.POST_NOTIFICATIONS) != PackageManager.PERMISSION_GRANTED) {
                needed.add(Manifest.permission.POST_NOTIFICATIONS)
            }
        } else {
            if (checkSelfPermission(Manifest.permission.READ_EXTERNAL_STORAGE) != PackageManager.PERMISSION_GRANTED) {
                needed.add(Manifest.permission.READ_EXTERNAL_STORAGE)
            }
            if (checkSelfPermission(Manifest.permission.WRITE_EXTERNAL_STORAGE) != PackageManager.PERMISSION_GRANTED) {
                needed.add(Manifest.permission.WRITE_EXTERNAL_STORAGE)
            }
        }
        if (needed.isNotEmpty()) {
            requestPermissionsLauncher.launch(needed.toTypedArray())
        }
    }

    // ──────────────────────── lifecycle ────────────────────────────────────────

    override fun onCreate(savedInstanceState: Bundle?) {
        instance = this
        super.onCreate(savedInstanceState)
        setImeEditorInfoFields(
            InputType.TYPE_CLASS_TEXT or InputType.TYPE_TEXT_FLAG_MULTI_LINE,
            EditorInfo.IME_ACTION_NONE,
            EditorInfo.IME_FLAG_NO_ENTER_ACTION,
        )
        applyImmersiveFullscreen()
        checkAndRequestPermissions()
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

    override fun onPause() {
        super.onPause()
        triggerNativeAutoSave()
    }

    override fun onSoftwareKeyboardVisibilityChanged(visible: Boolean) {
        super.onSoftwareKeyboardVisibilityChanged(visible)
        try {
            nativeOnKeyboardVisibilityChanged(visible)
        } catch (_: UnsatisfiedLinkError) {}
    }

    override fun onDestroy() {
        if (instance === this) instance = null
        super.onDestroy()
    }

    // ─────────────────────── key handling ──────────────────────────────────────

    override fun onEditorAction(action: Int) {
        super.onEditorAction(action)
        val now = SystemClock.uptimeMillis()
        val down = KeyEvent(now, now, KeyEvent.ACTION_DOWN, KeyEvent.KEYCODE_ENTER, 0)
        val up = KeyEvent(now, now, KeyEvent.ACTION_UP, KeyEvent.KEYCODE_ENTER, 0)
        onKeyDown(KeyEvent.KEYCODE_ENTER, down)
        onKeyUp(KeyEvent.KEYCODE_ENTER, up)
    }

    override fun onKeyDown(keyCode: Int, event: KeyEvent): Boolean {
        if (keyCode == KeyEvent.KEYCODE_VOLUME_UP || keyCode == KeyEvent.KEYCODE_VOLUME_DOWN) {
            return super.onKeyDown(keyCode, event)
        }
        val handled = super.onKeyDown(keyCode, event)
        if (!handled) {
            mSurfaceView?.dispatchKeyEvent(event)
        }
        return handled
    }

    override fun onKeyUp(keyCode: Int, event: KeyEvent): Boolean {
        if (keyCode == KeyEvent.KEYCODE_VOLUME_UP || keyCode == KeyEvent.KEYCODE_VOLUME_DOWN) {
            return super.onKeyUp(keyCode, event)
        }
        val handled = super.onKeyUp(keyCode, event)
        if (!handled) {
            mSurfaceView?.dispatchKeyEvent(event)
        }
        return handled
    }

    private fun triggerNativeAutoSave() {
        try {
            nativeTriggerAutoSave()
        } catch (e: UnsatisfiedLinkError) {
            Log.d(TAG, "nativeTriggerAutoSave not linked", e)
        }
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
     * Uses GameActivity's [mSurfaceView] so GameTextInput's InputConnection
     * remains active and receives commitText, setComposingText and character inputs.
     */
    fun showKeyboard() {
        val view = mSurfaceView ?: window.decorView
        view.isFocusable = true
        view.isFocusableInTouchMode = true
        if (!view.isFocused) {
            view.requestFocus()
        }
        setImeEditorInfoFields(
            InputType.TYPE_CLASS_TEXT or InputType.TYPE_TEXT_FLAG_MULTI_LINE,
            EditorInfo.IME_ACTION_NONE,
            EditorInfo.IME_FLAG_NO_ENTER_ACTION,
        )
        val imm = getSystemService(INPUT_METHOD_SERVICE) as? InputMethodManager
        imm?.restartInput(view)
        imm?.showSoftInput(view, InputMethodManager.SHOW_IMPLICIT)
        WindowCompat.getInsetsController(window, view).show(WindowInsetsCompat.Type.ime())
    }

    /** Called from Rust/JNI to dismiss the soft keyboard. */
    fun hideKeyboard() {
        val view = mSurfaceView ?: window.decorView
        val imm = getSystemService(INPUT_METHOD_SERVICE) as? InputMethodManager
        imm?.hideSoftInputFromWindow(view.windowToken, 0)
        WindowCompat.getInsetsController(window, view).hide(WindowInsetsCompat.Type.ime())
    }

    // ─────────────────────── open / save / autosave ─────────────────────────────

    private fun writeBytesToUri(uri: Uri, bytes: ByteArray): Boolean {
        return try {
            contentResolver.openOutputStream(uri)?.use { stream ->
                stream.write(bytes)
                stream.flush()
            } ?: return false
            Log.i(TAG, "Saved ${bytes.size} bytes to $uri")
            true
        } catch (e: Exception) {
            Log.e(TAG, "Failed to write bytes to $uri", e)
            false
        }
    }

    private fun saveInternalBackup(bytes: ByteArray) {
        if (bytes.isEmpty()) return
        try {
            File(filesDir, "autosave_backup.docx").writeBytes(bytes)
            Log.d(TAG, "Internal autosave backup updated (${bytes.size} bytes)")
        } catch (e: Exception) {
            Log.e(TAG, "Failed to write internal autosave backup", e)
        }
    }

    private fun saveOrOverwrite(name: String, bytes: ByteArray) {
        val uri = currentDocumentUri
        if (uri != null && writeBytesToUri(uri, bytes)) {
            saveInternalBackup(bytes)
            return
        }
        launchSave(name, bytes)
    }

    private fun performAutoSave(bytes: ByteArray) {
        if (bytes.isEmpty()) return
        saveInternalBackup(bytes)
        val uri = currentDocumentUri
        if (uri != null) {
            writeBytesToUri(uri, bytes)
        }
    }

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
        try {
            openDocument.launch(mimes)
        } catch (e: Exception) {
            Log.e(TAG, "Failed to launch OpenDocument picker", e)
        }
    }

    private fun launchSave(name: String, bytes: ByteArray) {
        pendingSaveBytes = bytes
        try {
            createDocument.launch(name)
        } catch (e: Exception) {
            Log.e(TAG, "Failed to launch CreateDocument picker", e)
        }
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
            act.runOnUiThread { act.saveOrOverwrite(name, bytes) }
        }

        @JvmStatic
        fun requestAutoSave(bytes: ByteArray) {
            val act = instance ?: return
            act.runOnUiThread { act.performAutoSave(bytes) }
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

        /** Trigger Rust engine to save document bytes for autosave on app pause. */
        @JvmStatic
        private external fun nativeTriggerAutoSave()

        /** Notify Rust engine when soft keyboard visibility changes. */
        @JvmStatic
        private external fun nativeOnKeyboardVisibilityChanged(visible: Boolean)
    }
}
