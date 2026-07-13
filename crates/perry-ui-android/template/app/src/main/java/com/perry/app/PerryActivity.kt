package com.perry.app

import android.app.Activity
import android.content.ComponentCallbacks2
import android.content.Intent
import android.content.pm.PackageManager
import android.os.Bundle
import android.widget.FrameLayout
import androidx.core.content.ContextCompat
import androidx.core.view.ViewCompat
import androidx.core.view.WindowInsetsCompat

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

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)

        // Switch from splash theme to normal theme before inflating layout
        setTheme(android.R.style.Theme_Material_Light_NoActionBar)

        // Keep content below the status / nav bars. Two layout regimes are in
        // play and the host padding has to follow whichever one is live:
        //   * API < 35 — the decor view fits system windows (explicitly on
        //     30-34, by default below that), so the content view is already
        //     inset and any padding we add here is just blank space.
        //   * API >= 35 — we target SDK 35, so Android 15 forces edge-to-edge
        //     and setDecorFitsSystemWindows() is a no-op. Nothing insets the
        //     content; without padding the UI draws under the system bars.
        // So pad from the insets we are actually handed rather than branching
        // on SDK_INT: the decor consumes them in the first regime (leaving us
        // to pad by zero) and passes them through in the second. This also
        // covers the nav bar, display cutouts and landscape, which a top-only
        // status_bar_height pad does not. getSafeAreaInsets() is no help here
        // — it still returns zeros on Android.
        if (android.os.Build.VERSION.SDK_INT >= 30) {
            @Suppress("DEPRECATION")
            window.setDecorFitsSystemWindows(true)
        }
        window.clearFlags(android.view.WindowManager.LayoutParams.FLAG_LAYOUT_NO_LIMITS)
        window.statusBarColor = android.graphics.Color.WHITE
        window.navigationBarColor = android.graphics.Color.WHITE
        if (android.os.Build.VERSION.SDK_INT >= android.os.Build.VERSION_CODES.M) {
            @Suppress("DEPRECATION")
            window.decorView.systemUiVisibility =
                android.view.View.SYSTEM_UI_FLAG_LIGHT_STATUS_BAR
        }

        rootLayout = FrameLayout(this)
        ViewCompat.setOnApplyWindowInsetsListener(rootLayout) { view, windowInsets ->
            val bars = windowInsets.getInsets(
                WindowInsetsCompat.Type.systemBars() or WindowInsetsCompat.Type.displayCutout()
            )
            view.setPadding(bars.left, bars.top, bars.right, bars.bottom)
            WindowInsetsCompat.CONSUMED
        }
        setContentView(rootLayout)

        // Store device locale in SharedPreferences so preferencesGet("AppleLanguages") works
        // cross-platform (matches iOS NSUserDefaults key)
        val locale = java.util.Locale.getDefault().language
        getSharedPreferences("perry_prefs", 0).edit()
            .putString("AppleLanguages", locale)
            .apply()

        // Initialize the bridge with this Activity
        PerryBridge.init(this, rootLayout)

        // #1138 — install optional `@perryts/google-auth` Kotlin
        // bridge if the package is present in the produced APK.
        // Uses reflection so the call is a no-op when the package
        // isn't installed (keeps the template build green for apps
        // that don't depend on Google Sign In).
        try {
            val cls = Class.forName("com.perryts.googleauth.PerryGoogleAuth")
            cls.getMethod("install", android.content.Context::class.java)
               .invoke(null, this)
        } catch (_: ClassNotFoundException) {
            // not installed — fine
        } catch (e: Throwable) {
            android.util.Log.w("PerryActivity",
                "PerryGoogleAuth.install failed: ${e.message}")
        }

        // Issue #583: capture the cold-start URL (if any). Tapping a
        // `myapp://…` link or a Universal-Link `https://yourdomain.com/…`
        // launches us with `intent.data` populated. The bridge holds the
        // URL until the JS module's `appOnOpenUrl` registers its handler.
        intent?.data?.toString()?.let { PerryBridge.onDeepLinkColdStart(it) }

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

    /**
     * Issue #583: foreground deep-link delivery. The OS calls this when the
     * Activity is already running (singleTop launchMode in the manifest)
     * and the user taps a deep link — the new URL arrives in this Intent
     * rather than via a fresh onCreate.
     */
    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        setIntent(intent)
        intent.data?.toString()?.let { PerryBridge.onDeepLinkForeground(it) }
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

    override fun onResume() {
        super.onResume()
        PerryBridge.forwardMapsLifecycle("resume")
    }

    override fun onPause() {
        super.onPause()
        PerryBridge.forwardMapsLifecycle("pause")
    }

    override fun onLowMemory() {
        super.onLowMemory()
        PerryBridge.forwardMapsLifecycle("lowMemory")
        // #6184: a system low-memory notice is the last warning before the
        // low-memory killer targets us → force a full collect + trigger clamp.
        forwardMemoryPressure(2)
    }

    override fun onTrimMemory(level: Int) {
        super.onTrimMemory(level)
        // #6184 (2026-07-09 GC audit): map Android trim levels onto the GC
        // memory-pressure API. RUNNING_CRITICAL (imminent kill while running)
        // and COMPLETE / MODERATE (background, high reclaim priority) are the
        // severe levels → full collect + trigger clamp (2). Every lighter
        // trim (RUNNING_LOW / RUNNING_MODERATE / BACKGROUND / UI_HIDDEN) is
        // advisory → minor collect (1). The constants aren't monotonic by
        // severity, so match the severe set explicitly rather than by range.
        val pressure = when (level) {
            ComponentCallbacks2.TRIM_MEMORY_RUNNING_CRITICAL,
            ComponentCallbacks2.TRIM_MEMORY_COMPLETE,
            ComponentCallbacks2.TRIM_MEMORY_MODERATE -> 2
            else -> 1
        }
        forwardMemoryPressure(pressure)
    }

    /// Forward an OS memory-pressure signal to the native GC. Guarded against
    /// UnsatisfiedLinkError because a warning can arrive before the native
    /// library finishes loading (permission gate / cold start).
    private fun forwardMemoryPressure(level: Int) {
        try {
            PerryBridge.nativeMemoryPressure(level)
        } catch (_: UnsatisfiedLinkError) {
            // Native library not loaded yet — the arena isn't live, nothing
            // to collect.
        }
    }

    override fun onDestroy() {
        super.onDestroy()
        PerryBridge.forwardMapsLifecycle("destroy")
        PerryBridge.nativeShutdown()
    }

    // Issue #1864: continuous keyboard events. Forward every hardware key
    // event to the native dispatcher BEFORE letting the system handle it,
    // so onKeyDown/onKeyUp fire even for keys the app doesn't otherwise
    // intercept. `super.dispatchKeyEvent` still runs so EditText / focus
    // navigation keep working.
    override fun dispatchKeyEvent(event: android.view.KeyEvent): Boolean {
        try {
            PerryBridge.nativeDispatchKey(
                event.keyCode,
                event.action,
                event.metaState,
                event.repeatCount,
            )
        } catch (_: UnsatisfiedLinkError) {
            // Native lib not loaded yet (key arrived during early startup).
        }
        return super.dispatchKeyEvent(event)
    }
}
