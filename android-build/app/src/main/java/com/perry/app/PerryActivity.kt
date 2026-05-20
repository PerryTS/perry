package com.perry.app

import android.app.Activity
import android.content.Intent
import android.content.pm.PackageManager
import android.os.Bundle
import android.widget.FrameLayout
import androidx.core.content.ContextCompat

/**
 * Minimal Activity that hosts a Perry-compiled native UI.
 *
 * Lifecycle:
 * 1. onCreate: create root FrameLayout, request runtime permissions
 * 2. After permissions granted: init PerryBridge, load native lib, spawn native thread
 * 3. Native thread runs the compiled TypeScript (which creates widgets via JNI)
 * 4. Native thread calls App() which blocks forever
 * 5. onDestroy: signal native thread to unpark and exit
 */
class PerryActivity : Activity() {

    private lateinit var rootLayout: FrameLayout
    private var nativeThread: Thread? = null
    private var nativeStarted = false

    companion object {
        private const val PERMISSION_REQUEST_CODE = 100
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        intent.data?.toString()?.takeIf { it.isNotEmpty() }?.let {
            PerryBridge.deliverDeepLinkUrl(it, "warm")
        }
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)

        // Switch from splash theme to normal theme before inflating layout
        setTheme(android.R.style.Theme_Material_Light_NoActionBar)

        // Go edge-to-edge (content under status/nav bars, matching iOS behavior)
        window.setFlags(
            android.view.WindowManager.LayoutParams.FLAG_LAYOUT_NO_LIMITS,
            android.view.WindowManager.LayoutParams.FLAG_LAYOUT_NO_LIMITS
        )

        rootLayout = FrameLayout(this)
        setContentView(rootLayout)

        // Store device locale in SharedPreferences so preferencesGet("AppleLanguages") works
        // cross-platform (matches iOS NSUserDefaults key)
        val locale = java.util.Locale.getDefault().language
        getSharedPreferences("perry_prefs", 0).edit()
            .putString("AppleLanguages", locale)
            .apply()

        // Initialize the bridge with this Activity
        PerryBridge.init(this, rootLayout)

        // Capture the cold-start deep-link URL (if any). The JS handler
        // hasn't been registered yet — PerryBridge.appOnOpenUrl() will
        // replay this through nativeInvokeDeepLinkCallback once it is.
        intent?.data?.toString()?.takeIf { it.isNotEmpty() }?.let {
            PerryBridge.deliverDeepLinkUrl(it, "cold-start")
        }

        // Request any dangerous runtime permissions declared in the manifest
        // before starting native code, so they're available when needed.
        val needed = getDangerousPermissionsToRequest()
        if (needed.isNotEmpty()) {
            requestPermissions(needed.toTypedArray(), PERMISSION_REQUEST_CODE)
        } else {
            startNative()
        }
    }

    /**
     * Find all dangerous permissions declared in the manifest that haven't been granted yet.
     * This covers RECORD_AUDIO, ACCESS_FINE_LOCATION, CAMERA, etc. — whatever the app declares.
     */
    private fun getDangerousPermissionsToRequest(): List<String> {
        return try {
            val info = packageManager.getPackageInfo(packageName, PackageManager.GET_PERMISSIONS)
            val requested = info.requestedPermissions ?: return emptyList()
            requested.filter { perm ->
                ContextCompat.checkSelfPermission(this, perm) != PackageManager.PERMISSION_GRANTED
            }
        } catch (e: Exception) {
            emptyList()
        }
    }

    override fun onRequestPermissionsResult(
        requestCode: Int,
        permissions: Array<out String>,
        grantResults: IntArray
    ) {
        super.onRequestPermissionsResult(requestCode, permissions, grantResults)

        when (requestCode) {
            PERMISSION_REQUEST_CODE -> {
                // Start native regardless of whether permissions were granted or denied.
                // The app code will handle missing permissions gracefully.
                startNative()
            }
            43 -> { // LOCATION_PERMISSION_REQUEST (legacy)
                val granted = grantResults.isNotEmpty() &&
                    grantResults[0] == PackageManager.PERMISSION_GRANTED
                PerryBridge.onLocationPermissionResult(granted)
            }
            44 -> { // AUDIO_PERMISSION_REQUEST (legacy)
                val granted = grantResults.isNotEmpty() &&
                    grantResults[0] == PackageManager.PERMISSION_GRANTED
                PerryBridge.onAudioPermissionResult(granted)
            }
            45 -> { // GEOLOCATION_PERMISSION_REQUEST (issue #552)
                val granted = grantResults.isNotEmpty() &&
                    grantResults.any { it == PackageManager.PERMISSION_GRANTED }
                PerryBridge.onGeolocationPermissionResult(granted)
            }
        }
    }

    @Deprecated("Required to wire pre-existing file dialog and the issue #552 image picker")
    override fun onActivityResult(requestCode: Int, resultCode: Int, data: Intent?) {
        super.onActivityResult(requestCode, resultCode, data)
        when (requestCode) {
            42 -> PerryBridge.onFileDialogResult(resultCode, data) // FILE_PICK_REQUEST
            46 -> PerryBridge.onImagePickerResult(resultCode, data) // IMAGE_PICK_REQUEST (#552)
        }
    }

    /**
     * Load the native library and start the perry-native thread.
     * Called after permissions are resolved (granted or denied).
     */
    private fun startNative() {
        if (nativeStarted) return
        nativeStarted = true

        // Load optional native libraries (e.g. hone-editor) before perry_app
        // so their JNI_OnLoad initializes before symbols are resolved
        try { System.loadLibrary("hone_editor_android") } catch (_: UnsatisfiedLinkError) {}

        // Load the native library (the compiled Perry app)
        System.loadLibrary("perry_app")

        // Initialize JNI cache on the UI thread first
        PerryBridge.nativeInit()

        // Spawn native init thread — this runs the compiled TypeScript main()
        nativeThread = Thread {
            // This calls the entry point of the compiled TypeScript.
            // It will create widgets via JNI, then call App() which blocks.
            PerryBridge.nativeMain()
        }.apply {
            name = "perry-native"
            isDaemon = true
            start()
        }
    }

    override fun onDestroy() {
        super.onDestroy()
        PerryBridge.nativeShutdown()
    }
}
