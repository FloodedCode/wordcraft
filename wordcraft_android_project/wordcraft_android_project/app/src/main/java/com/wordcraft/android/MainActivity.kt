package com.wordcraft.android

import android.annotation.SuppressLint
import android.app.Activity
import android.content.Context
import android.content.Intent
import android.net.Uri
import android.os.Bundle
import android.print.PrintAttributes
import android.print.PrintManager
import android.webkit.JavascriptInterface
import android.webkit.ValueCallback
import android.webkit.WebChromeClient
import android.webkit.WebResourceRequest
import android.webkit.WebSettings
import android.webkit.WebView
import android.webkit.WebViewClient
import android.widget.Toast
import java.io.OutputStream

/**
 * WordCraft for Android (v0.6.1) — 1:1 Open-Source Port of storytold/wordcraft
 *
 * Bridges the WordCraft Document & Ribbon Engine (9 Ribbon tabs, ECMA-376 .docx ZIP I/O,
 * Track Changes, Margin Comments, TOC/Citations, Mail Merge, Touch Drawing) with native
 * Android Storage Access Framework (SAF) and Android PrintManager PDF export.
 */
class MainActivity : Activity() {

    private lateinit var webView: WebView
    private var filePathCallback: ValueCallback<Array<Uri>>? = null
    private var pendingSaveBytes: ByteArray? = null

    companion object {
        private const val REQUEST_SELECT_FILE = 1001
        private const val REQUEST_CREATE_DOCX = 1002
    }

    @SuppressLint("SetJavaScriptEnabled")
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)

        webView = WebView(this).apply {
            settings.apply {
                javaScriptEnabled = true
                domStorageEnabled = true
                databaseEnabled = true
                allowFileAccess = true
                allowContentAccess = true
                allowFileAccessFromFileURLs = true
                allowUniversalAccessFromFileURLs = true
                builtInZoomControls = false
                displayZoomControls = false
                loadWithOverviewMode = true
                useWideViewPort = true
                cacheMode = WebSettings.LOAD_DEFAULT
            }

            addJavascriptInterface(AndroidDocumentBridge(this@MainActivity), "WordCraftAndroidBridge")

            webViewClient = object : WebViewClient() {
                override fun shouldOverrideUrlLoading(
                    view: WebView?,
                    request: WebResourceRequest?
                ): Boolean {
                    val url = request?.url?.toString() ?: return false
                    if (url.startsWith("file://")) return false
                    startActivity(Intent(Intent.ACTION_VIEW, Uri.parse(url)))
                    return true
                }
            }

            webChromeClient = object : WebChromeClient() {
                override fun onShowFileChooser(
                    webView: WebView?,
                    filePathCallback: ValueCallback<Array<Uri>>?,
                    fileChooserParams: FileChooserParams?
                ): Boolean {
                    this@MainActivity.filePathCallback?.onReceiveValue(null)
                    this@MainActivity.filePathCallback = filePathCallback
                    val intent = Intent(Intent.ACTION_GET_CONTENT).apply {
                        addCategory(Intent.CATEGORY_OPENABLE)
                        type = "*/*"
                        putExtra(
                            Intent.EXTRA_MIME_TYPES,
                            arrayOf(
                                "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
                                "text/markdown",
                                "text/html",
                                "text/plain",
                                "image/*"
                            )
                        )
                    }
                    startActivityForResult(Intent.createChooser(intent, "Dokument öffnen"), REQUEST_SELECT_FILE)
                    return true
                }
            }

            loadUrl("file:///android_asset/index.html")
        }

        setContentView(webView)
    }

    override fun onActivityResult(requestCode: Int, resultCode: Int, data: Intent?) {
        super.onActivityResult(requestCode, resultCode, data)
        if (requestCode == REQUEST_SELECT_FILE) {
            val results = if (resultCode == RESULT_OK && data?.data != null) {
                arrayOf(data.data!!)
            } else null
            filePathCallback?.onReceiveValue(results)
            filePathCallback = null
        } else if (requestCode == REQUEST_CREATE_DOCX && resultCode == RESULT_OK) {
            val uri = data?.data ?: return
            val bytes = pendingSaveBytes ?: return
            contentResolver.openOutputStream(uri)?.use { out: OutputStream ->
                out.write(bytes)
                out.flush()
            }
            pendingSaveBytes = null
            Toast.makeText(this, "Gespeichert: ${uri.lastPathSegment}", Toast.LENGTH_SHORT).show()
        }
    }

    override fun onBackPressed() {
        if (this::webView.isInitialized && webView.canGoBack()) {
            webView.goBack()
        } else {
            super.onBackPressed()
        }
    }

    inner class AndroidDocumentBridge(private val context: Context) {

        @JavascriptInterface
        fun printDocument(jobName: String) {
            runOnUiThread {
                val printManager = getSystemService(Context.PRINT_SERVICE) as PrintManager
                val printAdapter = webView.createPrintDocumentAdapter(jobName)
                printManager.print(jobName, printAdapter, PrintAttributes.Builder().build())
            }
        }

        @JavascriptInterface
        fun saveDocxNative(fileName: String, base64Data: String) {
            pendingSaveBytes = android.util.Base64.decode(base64Data, android.util.Base64.DEFAULT)
            val intent = Intent(Intent.ACTION_CREATE_DOCUMENT).apply {
                addCategory(Intent.CATEGORY_OPENABLE)
                type = "application/vnd.openxmlformats-officedocument.wordprocessingml.document"
                putExtra(Intent.EXTRA_TITLE, fileName)
            }
            startActivityForResult(intent, REQUEST_CREATE_DOCX)
        }
    }
}
