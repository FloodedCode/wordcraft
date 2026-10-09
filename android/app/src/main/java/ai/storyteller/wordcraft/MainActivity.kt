package ai.storyteller.wordcraft

import android.content.Intent
import android.net.Uri
import android.os.Bundle
import android.provider.OpenableColumns
import android.util.Log
import android.view.WindowManager
import androidx.activity.result.contract.ActivityResultContracts
import com.google.androidgamesdk.GameActivity

/**
 * GameActivity host for the Rust `libwordcraft.so` (eframe / egui desktop UI).
 *
 * Soft keyboard / IME comes from GameActivity. File open/save uses the Storage Access
 * Framework and relays bytes to Rust via [nativeOnFileOpened] / [requestSave].
 */
class MainActivity : GameActivity() {

    private var pendingSaveName: String? = null
    private var pendingSaveBytes: ByteArray? = null
    private var openPurpose: String = "document"

    private val openDocument = registerForActivityResult(
        ActivityResultContracts.OpenDocument(),
    ) { uri: Uri? ->
        if (uri == null) return@registerForActivityResult
        try {
            contentResolver.takePersistableUriPermission(
                uri,
                Intent.FLAG_GRANT_READ_URI_PERMISSION,
            )
        } catch (_: SecurityException) {
            // Not all providers support persistable permissions.
        }
        val name = displayName(uri) ?: "document"
        val bytes = contentResolver.openInputStream(uri)?.use { it.readBytes() }
        if (bytes != null) {
            nativeOnFileOpened(name, bytes)
        } else {
            Log.e(TAG, "failed to read $uri")
        }
    }

    private val createDocument = registerForActivityResult(
        ActivityResultContracts.CreateDocument("*/*"),
    ) { uri: Uri? ->
        val bytes = pendingSaveBytes
        pendingSaveBytes = null
        pendingSaveName = null
        if (uri == null || bytes == null) return@registerForActivityResult
        try {
            contentResolver.openOutputStream(uri)?.use { it.write(bytes) }
                ?: Log.e(TAG, "no output stream for $uri")
        } catch (e: Exception) {
            Log.e(TAG, "save failed", e)
        }
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        instance = this
        super.onCreate(savedInstanceState)
        window.addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
        // Open a document if launched via VIEW intent.
        intent?.data?.let { uri ->
            val name = displayName(uri) ?: "document"
            contentResolver.openInputStream(uri)?.use { stream ->
                nativeOnFileOpened(name, stream.readBytes())
            }
        }
    }

    override fun onDestroy() {
        if (instance === this) {
            instance = null
        }
        super.onDestroy()
    }

    private fun displayName(uri: Uri): String? {
        contentResolver.query(uri, arrayOf(OpenableColumns.DISPLAY_NAME), null, null, null)
            ?.use { cursor ->
                if (cursor.moveToFirst()) {
                    val idx = cursor.getColumnIndex(OpenableColumns.DISPLAY_NAME)
                    if (idx >= 0) return cursor.getString(idx)
                }
            }
        return uri.lastPathSegment
    }

    private fun launchOpen(purpose: String) {
        openPurpose = purpose
        val mime = if (purpose == "picture") {
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
        openDocument.launch(mime)
    }

    private fun launchSave(name: String, bytes: ByteArray) {
        pendingSaveName = name
        pendingSaveBytes = bytes
        createDocument.launch(name)
    }

    companion object {
        private const val TAG = "WordCraft"

        @Volatile
        private var instance: MainActivity? = null

        init {
            System.loadLibrary("wordcraft")
        }

        @JvmStatic
        fun requestOpen(purpose: String) {
            val act = instance
            if (act == null) {
                Log.w(TAG, "requestOpen: activity not ready")
                return
            }
            act.runOnUiThread { act.launchOpen(purpose) }
        }

        @JvmStatic
        fun requestSave(name: String, bytes: ByteArray) {
            val act = instance
            if (act == null) {
                Log.w(TAG, "requestSave: activity not ready")
                return
            }
            act.runOnUiThread { act.launchSave(name, bytes) }
        }

        @JvmStatic
        private external fun nativeOnFileOpened(name: String, bytes: ByteArray)
    }
}
