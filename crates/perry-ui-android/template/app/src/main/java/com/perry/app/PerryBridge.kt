package com.perry.app

import android.Manifest
import android.app.Activity
import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.graphics.Bitmap
import android.graphics.ImageFormat
import android.graphics.SurfaceTexture
import android.hardware.camera2.*
import android.location.LocationManager
import android.media.ImageReader
import android.net.Uri
import android.view.PixelCopy
import android.os.Handler
import android.os.HandlerThread
import android.os.Looper
import android.text.Editable
import android.text.TextWatcher
import android.util.Log
import android.util.TypedValue
import android.view.DragEvent
import android.view.MotionEvent
import android.view.Surface
import android.view.TextureView
import android.view.View
import android.widget.*
import androidx.core.app.ActivityCompat
import androidx.core.content.ContextCompat
import java.io.BufferedReader
import java.io.InputStreamReader
import java.nio.ByteBuffer
import java.util.concurrent.CountDownLatch

/**
 * Java-side JNI bridge for Perry UI.
 *
 * Provides:
 * - Activity/Context access for widget creation
 * - Callback wiring (OnClickListener, TextWatcher, etc.)
 * - Clipboard, file dialog, dp conversion
 * - runOnUiThreadBlocking for synchronous UI operations from native
 */
object PerryBridge {

    private lateinit var activity: Activity
    private lateinit var rootLayout: FrameLayout
    private val uiHandler = Handler(Looper.getMainLooper())

    // File dialog callback tracking
    private var pendingFileDialogKey: Long = 0
    private const val FILE_PICK_REQUEST = 42

    // Location callback tracking
    private var pendingLocationCallbackKey: Long = 0
    private const val LOCATION_PERMISSION_REQUEST = 43

    // Issue #552 geolocation + image picker
    private const val GEOLOCATION_PERMISSION_REQUEST = 45
    private const val IMAGE_PICK_REQUEST = 46
    private var pendingGeolocationSuccessKey: Long = 0
    private var pendingGeolocationErrorKey: Long = 0
    private var pendingGeolocationPermissionKey: Long = 0
    private var pendingImagePickerKey: Long = 0
    private var pendingImagePickerMaxCount: Int = 0
    private val watchListeners = mutableMapOf<Long, android.location.LocationListener>()
    private var nextWatchId: Long = 1L

    // Audio permission tracking
    private const val AUDIO_PERMISSION_REQUEST = 44
    private var audioPermissionGranted = false

    // Camera state
    private var cameraDevice: CameraDevice? = null
    private var captureSession: CameraCaptureSession? = null
    private var cameraThread: HandlerThread? = null
    private var cameraHandler: Handler? = null
    private var imageReader: ImageReader? = null
    private var cameraTextureView: TextureView? = null
    private var cameraFrozen = false
    @Volatile private var latestBitmap: Bitmap? = null
    @Volatile private var latestYuvFrame: YuvFrame? = null
    private var debugBitmapSaved = false

    data class YuvFrame(
        val width: Int, val height: Int,
        val yData: ByteArray, val uData: ByteArray, val vData: ByteArray,
        val yRowStride: Int, val uvRowStride: Int, val uvPixelStride: Int
    ) {
        fun sampleRgb(normX: Double, normY: Double): Triple<Int, Int, Int> {
            val px = (normX.coerceIn(0.0, 1.0) * (width - 1)).toInt().coerceIn(0, width - 1)
            val py = (normY.coerceIn(0.0, 1.0) * (height - 1)).toInt().coerceIn(0, height - 1)

            // Average 5x5 region
            val half = 2
            var rSum = 0L; var gSum = 0L; var bSum = 0L; var count = 0L
            for (sy in (py - half).coerceAtLeast(0)..(py + half).coerceAtMost(height - 1)) {
                for (sx in (px - half).coerceAtLeast(0)..(px + half).coerceAtMost(width - 1)) {
                    val yVal = (yData[sy * yRowStride + sx].toInt() and 0xFF)
                    val uvRow = sy / 2
                    val uvCol = sx / 2
                    val uIdx = uvRow * uvRowStride + uvCol * uvPixelStride
                    val vIdx = uIdx
                    val uVal = if (uIdx < uData.size) (uData[uIdx].toInt() and 0xFF) - 128 else 0
                    val vVal = if (vIdx < vData.size) (vData[vIdx].toInt() and 0xFF) - 128 else 0
                    rSum += (yVal + 1.370705 * vVal).toInt().coerceIn(0, 255)
                    gSum += (yVal - 0.337633 * uVal - 0.698001 * vVal).toInt().coerceIn(0, 255)
                    bSum += (yVal + 1.732446 * uVal).toInt().coerceIn(0, 255)
                    count++
                }
            }
            return Triple((rSum / count).toInt(), (gSum / count).toInt(), (bSum / count).toInt())
        }
    }
    private val TAG = "PerryCamera"

    fun init(activity: Activity, rootLayout: FrameLayout) {
        this.activity = activity
        this.rootLayout = rootLayout
    }

    // --- Activity access ---

    @JvmStatic
    fun getActivity(): Activity = activity

    // --- Content view ---

    @JvmStatic
    fun setContentView(view: View) {
        uiHandler.post {
            rootLayout.removeAllViews()
            rootLayout.addView(view, FrameLayout.LayoutParams(
                FrameLayout.LayoutParams.MATCH_PARENT,
                FrameLayout.LayoutParams.MATCH_PARENT
            ))
        }
    }

    // --- UI thread synchronization ---

    /**
     * Run a Runnable on the UI thread and block until it completes.
     * If already on the UI thread, run immediately.
     */
    @JvmStatic
    fun runOnUiThreadBlocking(callbackKey: Long) {
        if (Looper.myLooper() == Looper.getMainLooper()) {
            nativeInvokeCallback0(callbackKey)
        } else {
            val latch = CountDownLatch(1)
            uiHandler.post {
                nativeInvokeCallback0(callbackKey)
                latch.countDown()
            }
            latch.await()
        }
    }

    // --- dp conversion ---

    @JvmStatic
    fun dpToPx(dp: Float): Int {
        return TypedValue.applyDimension(
            TypedValue.COMPLEX_UNIT_DIP, dp,
            activity.resources.displayMetrics
        ).toInt()
    }

    // --- Button click callback ---

    @JvmStatic
    fun setOnClickCallback(view: View, callbackKey: Long) {
        view.setOnClickListener {
            nativeInvokeCallback0(callbackKey)
        }
    }

    // --- Click callback with argument (e.g. tab index) ---

    @JvmStatic
    fun setOnClickCallbackWithArg(view: View, callbackKey: Long, arg: Double) {
        view.setOnClickListener {
            nativeInvokeCallback1(callbackKey, arg)
        }
    }

    // --- Continuous pointer events (issue #1868) ---
    //
    // Installs a `View.OnTouchListener` that routes `ACTION_DOWN`,
    // `ACTION_MOVE`, `ACTION_UP` / `ACTION_CANCEL` to the three
    // pre-registered native callback keys. Coordinates are converted
    // from device pixels to dp so the JS side sees widget-local *points*
    // (top-left origin), matching the macOS / GTK4 / Windows backends.
    // `button` is always 0 for touch — multi-button mouse / stylus is
    // a planned follow-up via `MotionEvent.getButtonState()`.
    //
    // Returning `false` from the listener keeps the underlying control
    // — buttons, scroll containers, gesture detectors — receiving the
    // same events, so observing pointers never steals an existing tap.
    @JvmStatic
    fun setOnPointerCallbacks(
        view: View,
        downKey: Long,
        moveKey: Long,
        upKey: Long
    ) {
        val density = activity.resources.displayMetrics.density
        view.setOnTouchListener { _, ev ->
            val xDp = (ev.x / density).toDouble()
            val yDp = (ev.y / density).toDouble()
            when (ev.actionMasked) {
                android.view.MotionEvent.ACTION_DOWN ->
                    nativeInvokePointerCallback(downKey, xDp, yDp, 0)
                android.view.MotionEvent.ACTION_MOVE ->
                    nativeInvokePointerCallback(moveKey, xDp, yDp, 0)
                android.view.MotionEvent.ACTION_UP,
                android.view.MotionEvent.ACTION_CANCEL ->
                    nativeInvokePointerCallback(upKey, xDp, yDp, 0)
            }
            false
        }
    }

    // --- Issue #553: scroll-end callback with backpressure ---
    //
    // Attaches a scroll listener that fires `nativeInvokeCallback0(key)`
    // once when `getScrollY() + height >= contentBottom - thresholdPx`,
    // and re-arms only when the user scrolls back up past the threshold.
    // Used by both ScrollView and the future LazyVStack-on-Android path.
    private val scrollEndArmed = mutableMapOf<View, Boolean>()

    @JvmStatic
    fun setOnScrollEndCallback(view: View, callbackKey: Long, thresholdPx: Float) {
        scrollEndArmed[view] = true
        view.setOnScrollChangeListener(View.OnScrollChangeListener { v, _, scrollY, _, _ ->
            // ScrollView has exactly one child; bottom = child.height.
            val contentBottom = (v as? ScrollView)?.getChildAt(0)?.height ?: v.height
            val visibleBottom = scrollY + v.height
            val inZone = visibleBottom >= contentBottom - thresholdPx
            val armed = scrollEndArmed[v] ?: true
            when {
                inZone && armed -> {
                    scrollEndArmed[v] = false
                    nativeInvokeCallback0(callbackKey)
                }
                !inZone && !armed -> {
                    scrollEndArmed[v] = true
                }
            }
        })
    }

    // --- Drag & drop (issue #4773) ---

    // Install a drop destination on `view`. On ACTION_DROP the DragEvent's
    // ClipData is parsed into text / file-or-content URIs / web URLs and handed
    // to native, which builds the `{ text?, files?, urls? }` payload and invokes
    // the JS callback registered under `callbackKey`.
    @JvmStatic
    fun setOnDropCallback(view: View, callbackKey: Long) {
        view.setOnDragListener { _, event ->
            when (event.action) {
                DragEvent.ACTION_DROP -> {
                    var text: String? = null
                    val files = ArrayList<String>()
                    val urls = ArrayList<String>()
                    val clip = event.clipData
                    if (clip != null) {
                        for (i in 0 until clip.itemCount) {
                            val item = clip.getItemAt(i)
                            val t = item.text
                            if (t != null && text == null) text = t.toString()
                            val uri = item.uri
                            if (uri != null) {
                                val s = uri.toString()
                                when (uri.scheme) {
                                    "http", "https" -> urls.add(s)
                                    else -> files.add(s) // content:// / file://
                                }
                            }
                        }
                    }
                    nativeInvokeDropCallback(
                        callbackKey,
                        text,
                        if (files.isEmpty()) null else files.toTypedArray(),
                        if (urls.isEmpty()) null else urls.toTypedArray(),
                    )
                    true
                }
                // Accept the drag for the remaining lifecycle events.
                else -> true
            }
        }
    }

    // Install a drag source on `view`. A long-press starts the drag; for each
    // non-zero provider key, native returns the payload string for that
    // representation (text / file path / url), which is assembled into a
    // ClipData and handed to `startDragAndDrop`.
    @JvmStatic
    fun setDragSource(view: View, textKey: Long, fileKey: Long, urlKey: Long) {
        view.setOnLongClickListener { v ->
            val clip = buildDragClip(textKey, fileKey, urlKey)
                ?: return@setOnLongClickListener false
            v.startDragAndDrop(clip, View.DragShadowBuilder(v), null, 0)
            true
        }
    }

    private fun buildDragClip(textKey: Long, fileKey: Long, urlKey: Long): ClipData? {
        var clip: ClipData? = null
        if (textKey != 0L) {
            val s = nativeInvokeDragProvider(textKey)
            if (s.isNotEmpty()) clip = appendOrNew(clip, ClipData.newPlainText("text", s))
        }
        if (fileKey != 0L) {
            val s = nativeInvokeDragProvider(fileKey)
            if (s.isNotEmpty()) clip = appendOrNew(clip, ClipData.newRawUri("file", Uri.parse(s)))
        }
        if (urlKey != 0L) {
            val s = nativeInvokeDragProvider(urlKey)
            if (s.isNotEmpty()) clip = appendOrNew(clip, ClipData.newRawUri("url", Uri.parse(s)))
        }
        return clip
    }

    private fun appendOrNew(existing: ClipData?, fresh: ClipData): ClipData {
        if (existing == null) return fresh
        for (i in 0 until fresh.itemCount) existing.addItem(fresh.getItemAt(i))
        return existing
    }

    // --- Button styling ---

    @JvmStatic
    fun setButtonBorderless(view: View, bordered: Boolean) {
        if (view is Button) {
            if (!bordered) {
                // Set borderless style
                val attrs = intArrayOf(android.R.attr.selectableItemBackground)
                val ta = activity.obtainStyledAttributes(attrs)
                val bg = ta.getDrawable(0)
                ta.recycle()
                view.background = bg
            }
        }
    }

    // --- LinearLayout spacing ---

    /**
     * LinearLayout doesn't have a built-in spacing property.
     * We use showDividers with a transparent space divider.
     */
    @JvmStatic
    fun setLinearLayoutSpacing(layout: LinearLayout, spacingPx: Int) {
        if (spacingPx > 0) {
            // Use divider with padding to simulate spacing
            layout.showDividers = LinearLayout.SHOW_DIVIDER_MIDDLE
            val divider = android.graphics.drawable.ShapeDrawable()
            divider.intrinsicWidth = spacingPx
            divider.intrinsicHeight = spacingPx
            divider.paint.color = android.graphics.Color.TRANSPARENT
            layout.dividerDrawable = divider
        }
    }

    // --- EditText text changed callback ---

    @JvmStatic
    fun setTextChangedCallback(editText: EditText, callbackKey: Long) {
        editText.addTextChangedListener(object : TextWatcher {
            override fun beforeTextChanged(s: CharSequence?, start: Int, count: Int, after: Int) {}
            override fun onTextChanged(s: CharSequence?, start: Int, before: Int, count: Int) {}
            override fun afterTextChanged(s: Editable?) {
                val text = s?.toString() ?: ""
                nativeInvokeCallback1WithString(callbackKey, text)
            }
        })
    }

    // --- Switch/Toggle callback ---

    @JvmStatic
    fun setOnCheckedChangeCallback(button: CompoundButton, callbackKey: Long) {
        button.setOnCheckedChangeListener { _, isChecked ->
            // NaN-boxed TAG_TRUE = 0x7FFC_0000_0000_0004, TAG_FALSE = 0x7FFC_0000_0000_0003
            val value = if (isChecked) {
                java.lang.Double.longBitsToDouble(0x7FFC_0000_0000_0004L)
            } else {
                java.lang.Double.longBitsToDouble(0x7FFC_0000_0000_0003L)
            }
            nativeInvokeCallback1(callbackKey, value)
        }
    }

    // --- SeekBar callback ---

    @JvmStatic
    fun setSeekBarCallback(seekBar: SeekBar, callbackKey: Long, min: Double, max: Double) {
        // Store min in tag for setSeekBarValue
        seekBar.tag = doubleArrayOf(min, max)
        seekBar.setOnSeekBarChangeListener(object : SeekBar.OnSeekBarChangeListener {
            override fun onProgressChanged(bar: SeekBar?, progress: Int, fromUser: Boolean) {
                if (fromUser) {
                    // Convert integer progress back to float value
                    val value = min + (progress.toDouble() / 100.0)
                    nativeInvokeCallback1(callbackKey, value)
                }
            }
            override fun onStartTrackingTouch(bar: SeekBar?) {}
            override fun onStopTrackingTouch(bar: SeekBar?) {}
        })
    }

    @JvmStatic
    fun setSeekBarValue(seekBar: SeekBar, value: Double) {
        val range = seekBar.tag as? DoubleArray ?: return
        val min = range[0]
        val progress = ((value - min) * 100.0).toInt()
        seekBar.progress = progress
    }

    // --- Context menu ---

    @JvmStatic
    fun setContextMenu(view: View, menuHandle: Long) {
        view.setOnLongClickListener {
            val popup = PopupMenu(activity, view)
            val itemCount = nativeGetMenuItemCount(menuHandle)
            for (i in 0 until itemCount) {
                val title = nativeGetMenuItemTitle(menuHandle, i)
                popup.menu.add(0, i, i, title)
            }
            popup.setOnMenuItemClickListener { item ->
                nativeMenuItemSelected(menuHandle, item.itemId)
                true
            }
            popup.show()
            true
        }
    }

    // --- Clipboard ---

    @JvmStatic
    fun clipboardRead(): String? {
        val cm = activity.getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager
        val clip = cm.primaryClip ?: return null
        if (clip.itemCount == 0) return null
        return clip.getItemAt(0).text?.toString()
    }

    @JvmStatic
    fun clipboardWrite(text: String) {
        val cm = activity.getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager
        val clip = ClipData.newPlainText("perry", text)
        cm.setPrimaryClip(clip)
    }

    // --- File dialog ---

    @JvmStatic
    fun openFileDialog(callbackKey: Long) {
        pendingFileDialogKey = callbackKey
        val intent = Intent(Intent.ACTION_OPEN_DOCUMENT).apply {
            addCategory(Intent.CATEGORY_OPENABLE)
            type = "*/*"
        }
        activity.startActivityForResult(intent, FILE_PICK_REQUEST)
    }

    /**
     * Called from PerryActivity.onActivityResult when a file is picked.
     */
    fun onFileDialogResult(resultCode: Int, data: Intent?) {
        if (resultCode == Activity.RESULT_OK && data?.data != null) {
            val uri: Uri = data.data!!
            try {
                val content = activity.contentResolver.openInputStream(uri)?.use { stream ->
                    BufferedReader(InputStreamReader(stream)).readText()
                }
                nativeFileDialogResult(pendingFileDialogKey, content)
            } catch (e: Exception) {
                nativeFileDialogResult(pendingFileDialogKey, null)
            }
        } else {
            nativeFileDialogResult(pendingFileDialogKey, null)
        }
    }

    /**
     * Helper: invoke a callback with a string argument.
     * Converts the string to a NaN-boxed Perry string via JNI.
     */
    private fun nativeInvokeCallback1WithString(key: Long, text: String) {
        // This calls back into native code which will:
        // 1. Convert the Java string to a Perry runtime string
        // 2. NaN-box it
        // 3. Invoke the closure with the NaN-boxed string
        nativeInvokeCallbackWithString(key, text)
    }

    // --- Location ---

    @JvmStatic
    fun requestLocation(callbackKey: Long) {
        pendingLocationCallbackKey = callbackKey
        if (ContextCompat.checkSelfPermission(activity, Manifest.permission.ACCESS_FINE_LOCATION)
            == PackageManager.PERMISSION_GRANTED) {
            fetchLastLocation(callbackKey)
        } else {
            ActivityCompat.requestPermissions(
                activity,
                arrayOf(Manifest.permission.ACCESS_FINE_LOCATION, Manifest.permission.ACCESS_COARSE_LOCATION),
                LOCATION_PERMISSION_REQUEST
            )
        }
    }

    private fun fetchLastLocation(callbackKey: Long) {
        try {
            val lm = activity.getSystemService(Context.LOCATION_SERVICE) as LocationManager
            @Suppress("MissingPermission")
            val loc = lm.getLastKnownLocation(LocationManager.GPS_PROVIDER)
                ?: lm.getLastKnownLocation(LocationManager.NETWORK_PROVIDER)
            if (loc != null) {
                nativeInvokeCallback2(callbackKey, loc.latitude, loc.longitude)
            } else {
                // No cached location — request a single update
                @Suppress("MissingPermission")
                lm.requestSingleUpdate(LocationManager.NETWORK_PROVIDER,
                    object : android.location.LocationListener {
                        override fun onLocationChanged(location: android.location.Location) {
                            nativeInvokeCallback2(callbackKey, location.latitude, location.longitude)
                        }
                        @Deprecated("Deprecated in Java")
                        override fun onStatusChanged(provider: String?, status: Int, extras: android.os.Bundle?) {}
                        override fun onProviderEnabled(provider: String) {}
                        override fun onProviderDisabled(provider: String) {
                            // NaN signals failure
                            nativeInvokeCallback2(callbackKey, Double.NaN, Double.NaN)
                        }
                    },
                    Looper.getMainLooper()
                )
            }
        } catch (e: Exception) {
            nativeInvokeCallback2(callbackKey, Double.NaN, Double.NaN)
        }
    }

    fun onLocationPermissionResult(granted: Boolean) {
        if (granted) {
            fetchLastLocation(pendingLocationCallbackKey)
        } else {
            nativeInvokeCallback2(pendingLocationCallbackKey, Double.NaN, Double.NaN)
        }
    }

    // --- Geolocation (issue #552) ---

    @JvmStatic
    fun requestGeolocationGetCurrent(successKey: Long, errorKey: Long) {
        pendingGeolocationSuccessKey = successKey
        pendingGeolocationErrorKey = errorKey
        if (Looper.myLooper() != Looper.getMainLooper()) {
            uiHandler.post { requestGeolocationGetCurrent(successKey, errorKey) }
            return
        }
        if (ContextCompat.checkSelfPermission(activity, Manifest.permission.ACCESS_FINE_LOCATION)
                == PackageManager.PERMISSION_GRANTED ||
            ContextCompat.checkSelfPermission(activity, Manifest.permission.ACCESS_COARSE_LOCATION)
                == PackageManager.PERMISSION_GRANTED) {
            fetchLocationOnce(successKey, errorKey)
        } else {
            ActivityCompat.requestPermissions(
                activity,
                arrayOf(Manifest.permission.ACCESS_FINE_LOCATION, Manifest.permission.ACCESS_COARSE_LOCATION),
                GEOLOCATION_PERMISSION_REQUEST
            )
        }
    }

    private fun fetchLocationOnce(successKey: Long, errorKey: Long) {
        try {
            val lm = activity.getSystemService(Context.LOCATION_SERVICE) as LocationManager
            // Try last-known position first (cheap, often sufficient).
            @Suppress("MissingPermission")
            val cached = lm.getLastKnownLocation(LocationManager.GPS_PROVIDER)
                ?: lm.getLastKnownLocation(LocationManager.NETWORK_PROVIDER)
            if (cached != null) {
                nativeInvokeCallback4(successKey, cached.latitude, cached.longitude,
                    cached.accuracy.toDouble(), cached.time.toDouble())
                return
            }
            // No cached fix — request a single update on the network provider
            // (least battery-hungry; GPS as fallback).
            val provider = when {
                lm.isProviderEnabled(LocationManager.NETWORK_PROVIDER) -> LocationManager.NETWORK_PROVIDER
                lm.isProviderEnabled(LocationManager.GPS_PROVIDER) -> LocationManager.GPS_PROVIDER
                else -> {
                    nativeInvokeCallbackWithString(errorKey, "no-provider-available")
                    return
                }
            }
            @Suppress("MissingPermission")
            lm.requestSingleUpdate(provider,
                object : android.location.LocationListener {
                    override fun onLocationChanged(location: android.location.Location) {
                        nativeInvokeCallback4(successKey, location.latitude, location.longitude,
                            location.accuracy.toDouble(), location.time.toDouble())
                    }
                    @Deprecated("Deprecated in Java")
                    override fun onStatusChanged(provider: String?, status: Int, extras: android.os.Bundle?) {}
                    override fun onProviderEnabled(provider: String) {}
                    override fun onProviderDisabled(provider: String) {
                        nativeInvokeCallbackWithString(errorKey, "provider-disabled")
                    }
                },
                Looper.getMainLooper()
            )
        } catch (e: SecurityException) {
            nativeInvokeCallbackWithString(errorKey, "permission-denied")
        } catch (e: Exception) {
            nativeInvokeCallbackWithString(errorKey, e.message ?: "location-error")
        }
    }

    @JvmStatic
    fun requestGeolocationWatch(callbackKey: Long): Long {
        if (Looper.myLooper() != Looper.getMainLooper()) {
            // We need a synchronous return value, so block briefly on the
            // UI thread to register the listener.
            val latch = CountDownLatch(1)
            var result: Long = 0
            uiHandler.post {
                result = registerWatchListener(callbackKey)
                latch.countDown()
            }
            latch.await()
            return result
        }
        return registerWatchListener(callbackKey)
    }

    private fun registerWatchListener(callbackKey: Long): Long {
        if (ContextCompat.checkSelfPermission(activity, Manifest.permission.ACCESS_FINE_LOCATION)
                != PackageManager.PERMISSION_GRANTED &&
            ContextCompat.checkSelfPermission(activity, Manifest.permission.ACCESS_COARSE_LOCATION)
                != PackageManager.PERMISSION_GRANTED) {
            // No permission — return 0 (caller should request permission first).
            return 0L
        }
        val id = nextWatchId++
        val listener = object : android.location.LocationListener {
            override fun onLocationChanged(location: android.location.Location) {
                nativeInvokeCallback4(callbackKey, location.latitude, location.longitude,
                    location.accuracy.toDouble(), location.time.toDouble())
            }
            @Deprecated("Deprecated in Java")
            override fun onStatusChanged(provider: String?, status: Int, extras: android.os.Bundle?) {}
            override fun onProviderEnabled(provider: String) {}
            override fun onProviderDisabled(provider: String) {}
        }
        try {
            val lm = activity.getSystemService(Context.LOCATION_SERVICE) as LocationManager
            val provider = when {
                lm.isProviderEnabled(LocationManager.NETWORK_PROVIDER) -> LocationManager.NETWORK_PROVIDER
                lm.isProviderEnabled(LocationManager.GPS_PROVIDER) -> LocationManager.GPS_PROVIDER
                else -> return 0L
            }
            @Suppress("MissingPermission")
            lm.requestLocationUpdates(provider, 0L, 0f, listener, Looper.getMainLooper())
            watchListeners[id] = listener
            return id
        } catch (e: Exception) {
            return 0L
        }
    }

    @JvmStatic
    fun stopGeolocationWatch(id: Long) {
        if (Looper.myLooper() != Looper.getMainLooper()) {
            uiHandler.post { stopGeolocationWatch(id) }
            return
        }
        val listener = watchListeners.remove(id) ?: return
        try {
            val lm = activity.getSystemService(Context.LOCATION_SERVICE) as LocationManager
            lm.removeUpdates(listener)
        } catch (_: Exception) {}
    }

    @JvmStatic
    fun requestGeolocationPermission(callbackKey: Long) {
        pendingGeolocationPermissionKey = callbackKey
        if (Looper.myLooper() != Looper.getMainLooper()) {
            uiHandler.post { requestGeolocationPermission(callbackKey) }
            return
        }
        val granted =
            ContextCompat.checkSelfPermission(activity, Manifest.permission.ACCESS_FINE_LOCATION) ==
                PackageManager.PERMISSION_GRANTED ||
            ContextCompat.checkSelfPermission(activity, Manifest.permission.ACCESS_COARSE_LOCATION) ==
                PackageManager.PERMISSION_GRANTED
        if (granted) {
            nativeInvokeCallbackWithString(callbackKey, "granted")
        } else {
            // Request via standard permission flow; result routed through
            // PerryActivity.onRequestPermissionsResult → onGeolocationPermissionResult.
            ActivityCompat.requestPermissions(
                activity,
                arrayOf(Manifest.permission.ACCESS_FINE_LOCATION, Manifest.permission.ACCESS_COARSE_LOCATION),
                GEOLOCATION_PERMISSION_REQUEST
            )
        }
    }

    /** Routed from PerryActivity.onRequestPermissionsResult. */
    fun onGeolocationPermissionResult(granted: Boolean) {
        if (pendingGeolocationPermissionKey != 0L) {
            nativeInvokeCallbackWithString(
                pendingGeolocationPermissionKey,
                if (granted) "granted" else "denied"
            )
            pendingGeolocationPermissionKey = 0L
        }
        // If a getCurrent was waiting on permission, fulfill it now.
        if (pendingGeolocationSuccessKey != 0L) {
            if (granted) {
                fetchLocationOnce(pendingGeolocationSuccessKey, pendingGeolocationErrorKey)
            } else {
                nativeInvokeCallbackWithString(pendingGeolocationErrorKey, "permission-denied")
            }
            pendingGeolocationSuccessKey = 0L
            pendingGeolocationErrorKey = 0L
        }
    }

    // --- Network reachability (issue #582) ---

    private val networkListeners = mutableMapOf<Long, Long>()  // listenerId -> callbackKey
    private var nextNetworkListenerId: Long = 1L
    private var networkCallback: android.net.ConnectivityManager.NetworkCallback? = null
    private var lastNetworkConnected: Boolean = false
    private var lastNetworkKind: String = "unknown"
    private var networkInitialized: Boolean = false

    private fun classifyNetwork(caps: android.net.NetworkCapabilities?): Pair<Boolean, String> {
        if (caps == null) return Pair(false, "none")
        val internet = caps.hasCapability(android.net.NetworkCapabilities.NET_CAPABILITY_INTERNET)
        val validated =
            caps.hasCapability(android.net.NetworkCapabilities.NET_CAPABILITY_VALIDATED)
        if (!internet) return Pair(false, "none")
        val kind = when {
            caps.hasTransport(android.net.NetworkCapabilities.TRANSPORT_WIFI) -> "wifi"
            caps.hasTransport(android.net.NetworkCapabilities.TRANSPORT_CELLULAR) -> "cellular"
            caps.hasTransport(android.net.NetworkCapabilities.TRANSPORT_ETHERNET) -> "ethernet"
            else -> "unknown"
        }
        // Treat presence-of-INTERNET as connected; VALIDATED is a stronger
        // guarantee but absence (e.g. captive portal) shouldn't make us claim
        // offline — leave that distinction to the app layer.
        return Pair(validated || internet, kind)
    }

    private fun ensureNetworkMonitorStarted() {
        if (networkCallback != null) return
        val cm = activity.getSystemService(Context.CONNECTIVITY_SERVICE)
                as? android.net.ConnectivityManager ?: return

        // Seed cached state from the active network so the first
        // networkGetStatus call has a real value to return without waiting
        // for the first callback fire.
        try {
            val active = cm.activeNetwork
            val caps = if (active != null) cm.getNetworkCapabilities(active) else null
            val (c, k) = classifyNetwork(caps)
            lastNetworkConnected = c
            lastNetworkKind = k
            networkInitialized = true
        } catch (_: Exception) {}

        val cb = object : android.net.ConnectivityManager.NetworkCallback() {
            override fun onCapabilitiesChanged(
                network: android.net.Network,
                caps: android.net.NetworkCapabilities
            ) {
                val (c, k) = classifyNetwork(caps)
                deliver(c, k)
            }
            override fun onLost(network: android.net.Network) {
                deliver(false, "none")
            }
            override fun onAvailable(network: android.net.Network) {
                // onCapabilitiesChanged usually fires right after onAvailable
                // with the real type — stay quiet here unless we have nothing.
                if (!networkInitialized) {
                    deliver(true, "unknown")
                }
            }
            private fun deliver(connected: Boolean, kind: String) {
                lastNetworkConnected = connected
                lastNetworkKind = kind
                networkInitialized = true
                val snapshot = networkListeners.values.toList()
                uiHandler.post {
                    for (key in snapshot) {
                        nativeInvokeNetworkCallback(key, connected, kind)
                    }
                }
            }
        }
        try {
            cm.registerDefaultNetworkCallback(cb)
            networkCallback = cb
        } catch (_: Exception) {
            // ACCESS_NETWORK_STATE missing or registration failed — leave
            // the cache at unknown / disconnected; getStatus still resolves.
        }
    }

    @JvmStatic
    fun networkGetStatus(callbackKey: Long) {
        if (Looper.myLooper() != Looper.getMainLooper()) {
            uiHandler.post { networkGetStatus(callbackKey) }
            return
        }
        ensureNetworkMonitorStarted()
        nativeInvokeNetworkCallback(callbackKey, lastNetworkConnected, lastNetworkKind)
    }

    @JvmStatic
    fun networkOnChange(callbackKey: Long): Long {
        if (Looper.myLooper() != Looper.getMainLooper()) {
            val latch = CountDownLatch(1)
            var result: Long = 0
            uiHandler.post {
                result = networkOnChange(callbackKey)
                latch.countDown()
            }
            latch.await()
            return result
        }
        ensureNetworkMonitorStarted()
        val id = nextNetworkListenerId++
        networkListeners[id] = callbackKey
        return id
    }

    @JvmStatic
    fun networkStopOnChange(id: Long) {
        if (Looper.myLooper() != Looper.getMainLooper()) {
            uiHandler.post { networkStopOnChange(id) }
            return
        }
        networkListeners.remove(id)
    }

    // ─── Deep links (issue #583) ───────────────────────────────────────────
    //
    // Single-handler model matching iOS / macOS — `appOnOpenUrl` replaces
    // the previous handler. `pendingColdStartUrl` is captured before the
    // handler is registered (cold-start URL arrives via the Activity's
    // onCreate before the JS module's appOnOpenUrl call has run); the
    // first `appOnOpenUrl` drains it. `lastLaunchUrl` is the cached cold-
    // start URL exposed via `appGetLaunchUrl`; cleared once the handler
    // has consumed it so a re-launch's URL doesn't shadow.

    private var deepLinkHandlerKey: Long = 0L
    private var pendingColdStartUrl: String? = null
    private var lastLaunchUrl: String = ""
    private var appLaunched: Boolean = false

    /// Called from the Activity's `onCreate` after `intent.data` is read.
    /// If the JS handler is already registered (rare but possible — a
    /// quick-spawned native thread might race ahead), fires immediately;
    /// otherwise caches until `appOnOpenUrl` arrives.
    @JvmStatic
    fun onDeepLinkColdStart(url: String?) {
        if (url.isNullOrEmpty()) return
        if (Looper.myLooper() != Looper.getMainLooper()) {
            uiHandler.post { onDeepLinkColdStart(url) }
            return
        }
        lastLaunchUrl = url
        if (deepLinkHandlerKey != 0L) {
            nativeInvokeDeepLinkCallback(deepLinkHandlerKey, url, "cold-start")
        } else {
            pendingColdStartUrl = url
        }
    }

    /// Called from `onNewIntent` when the OS hands the running Activity
    /// a fresh URL.
    @JvmStatic
    fun onDeepLinkForeground(url: String?) {
        if (url.isNullOrEmpty()) return
        if (Looper.myLooper() != Looper.getMainLooper()) {
            uiHandler.post { onDeepLinkForeground(url) }
            return
        }
        lastLaunchUrl = url
        if (deepLinkHandlerKey != 0L) {
            nativeInvokeDeepLinkCallback(deepLinkHandlerKey, url, "foreground")
        }
        // No handler — drop. Foreground deliveries can't be replayed
        // without a listener; stashing them would mask logic bugs in user
        // code (forgetting to register the handler).
    }

    @JvmStatic
    fun appOnOpenUrl(callbackKey: Long) {
        if (Looper.myLooper() != Looper.getMainLooper()) {
            uiHandler.post { appOnOpenUrl(callbackKey) }
            return
        }
        deepLinkHandlerKey = callbackKey
        val pending = pendingColdStartUrl
        pendingColdStartUrl = null
        if (pending != null) {
            nativeInvokeDeepLinkCallback(callbackKey, pending, "cold-start")
        }
    }

    @JvmStatic
    fun appGetLaunchUrl(): String {
        return lastLaunchUrl
    }

    // --- Image picker (issue #552) ---

    @JvmStatic
    fun requestImagePickerPick(maxCount: Int, allowMultiple: Boolean, callbackKey: Long) {
        if (Looper.myLooper() != Looper.getMainLooper()) {
            uiHandler.post { requestImagePickerPick(maxCount, allowMultiple, callbackKey) }
            return
        }
        pendingImagePickerKey = callbackKey
        pendingImagePickerMaxCount = maxCount

        // Build the picker intent. Photo Picker (API 33+) is the modern,
        // privacy-preserving path. Older devices fall back to ACTION_GET_CONTENT.
        val intent = if (android.os.Build.VERSION.SDK_INT >= 33) {
            Intent(android.provider.MediaStore.ACTION_PICK_IMAGES).apply {
                type = "image/*"
                if (allowMultiple) {
                    val extraName = android.provider.MediaStore.EXTRA_PICK_IMAGES_MAX
                    val limit = if (maxCount in 1..10) maxCount else 10
                    putExtra(extraName, limit)
                }
            }
        } else {
            Intent(Intent.ACTION_GET_CONTENT).apply {
                type = "image/*"
                addCategory(Intent.CATEGORY_OPENABLE)
                if (allowMultiple) {
                    putExtra(Intent.EXTRA_ALLOW_MULTIPLE, true)
                }
            }
        }
        try {
            activity.startActivityForResult(intent, IMAGE_PICK_REQUEST)
        } catch (e: Exception) {
            // No suitable activity — return empty array.
            nativeInvokeCallbackWithStringArray(callbackKey, emptyArray())
            pendingImagePickerKey = 0L
        }
    }

    /**
     * Routed from PerryActivity.onActivityResult. Copies each selected URI's
     * content into a fresh file under the app's cache directory and returns
     * the absolute paths (so the user can fs.readFileSync them or upload).
     */
    fun onImagePickerResult(resultCode: Int, data: Intent?) {
        val key = pendingImagePickerKey
        val max = pendingImagePickerMaxCount
        pendingImagePickerKey = 0L
        if (key == 0L) return

        if (resultCode != Activity.RESULT_OK || data == null) {
            nativeInvokeCallbackWithStringArray(key, emptyArray())
            return
        }

        val uris = mutableListOf<Uri>()
        val clip = data.clipData
        if (clip != null) {
            val count = clip.itemCount
            for (i in 0 until count) {
                if (max in 1..uris.size) break
                clip.getItemAt(i)?.uri?.let { uris.add(it) }
            }
        } else {
            data.data?.let { uris.add(it) }
        }

        val paths = mutableListOf<String>()
        val cacheDir = activity.cacheDir
        for ((i, uri) in uris.withIndex()) {
            try {
                val ext = guessImageExtension(uri)
                val out = java.io.File(cacheDir, "perry_pick_${System.currentTimeMillis()}_$i.$ext")
                activity.contentResolver.openInputStream(uri)?.use { input ->
                    java.io.FileOutputStream(out).use { output ->
                        input.copyTo(output)
                    }
                }
                if (out.exists() && out.length() > 0) {
                    paths.add(out.absolutePath)
                }
            } catch (_: Exception) {
                // skip this URI; user gets the others
            }
        }
        nativeInvokeCallbackWithStringArray(key, paths.toTypedArray())
    }

    private fun guessImageExtension(uri: Uri): String {
        val mime = activity.contentResolver.getType(uri) ?: return "jpg"
        return when (mime) {
            "image/jpeg" -> "jpg"
            "image/png" -> "png"
            "image/gif" -> "gif"
            "image/webp" -> "webp"
            "image/heic" -> "heic"
            "image/heif" -> "heif"
            "image/bmp" -> "bmp"
            else -> "jpg"
        }
    }

    // --- Background tasks (issue #538) — WorkManager ---

    /** Map identifier → callback key, written from native registerTask, read
     *  by PerryBackgroundWorker.doWork to invoke the right Perry handler. */
    private val backgroundHandlerKeys = mutableMapOf<String, Long>()

    @JvmStatic
    fun backgroundRegisterTask(identifier: String, callbackKey: Long) {
        backgroundHandlerKeys[identifier] = callbackKey
    }

    @JvmStatic
    fun backgroundLookupCallbackKey(identifier: String): Long {
        return backgroundHandlerKeys[identifier] ?: 0L
    }

    @JvmStatic
    fun backgroundSchedule(
        identifier: String,
        kind: String,
        earliestStartMs: Double,
        requiresNetwork: Boolean,
        requiresCharging: Boolean
    ) {
        try {
            val constraintsCls = Class.forName("androidx.work.Constraints\$Builder")
            val constraintsBuilder = constraintsCls.getConstructor().newInstance()
            if (requiresNetwork) {
                val networkTypeCls = Class.forName("androidx.work.NetworkType")
                val connected = networkTypeCls.getField("CONNECTED").get(null)
                constraintsCls.getMethod("setRequiredNetworkType", networkTypeCls)
                    .invoke(constraintsBuilder, connected)
            }
            if (requiresCharging) {
                constraintsCls.getMethod("setRequiresCharging", Boolean::class.javaPrimitiveType)
                    .invoke(constraintsBuilder, true)
            }
            val constraints = constraintsCls.getMethod("build").invoke(constraintsBuilder)

            val workerCls = Class.forName("com.perry.app.PerryBackgroundWorker")
            val builderCls = Class.forName("androidx.work.OneTimeWorkRequest\$Builder")
            val builder = builderCls.getConstructor(Class::class.java).newInstance(workerCls)

            // Stash the identifier in inputData so the Worker knows which handler to invoke.
            val dataBuilderCls = Class.forName("androidx.work.Data\$Builder")
            val dataBuilder = dataBuilderCls.getConstructor().newInstance()
            dataBuilderCls.getMethod("putString", String::class.java, String::class.java)
                .invoke(dataBuilder, "identifier", identifier)
            val data = dataBuilderCls.getMethod("build").invoke(dataBuilder)
            builderCls.getMethod("setInputData", Class.forName("androidx.work.Data"))
                .invoke(builder, data)

            builderCls.getMethod("setConstraints", Class.forName("androidx.work.Constraints"))
                .invoke(builder, constraints)

            if (earliestStartMs > 0.0) {
                val nowMs = System.currentTimeMillis().toDouble()
                val delayMs = (earliestStartMs - nowMs).toLong().coerceAtLeast(0L)
                builderCls.getMethod(
                    "setInitialDelay",
                    Long::class.javaPrimitiveType,
                    java.util.concurrent.TimeUnit::class.java
                ).invoke(builder, delayMs, java.util.concurrent.TimeUnit.MILLISECONDS)
            }

            val request = builderCls.getMethod("build").invoke(builder)
            val workManagerCls = Class.forName("androidx.work.WorkManager")
            val workManager = workManagerCls.getMethod("getInstance", Context::class.java)
                .invoke(null, activity)
            val policyCls = Class.forName("androidx.work.ExistingWorkPolicy")
            val replace = policyCls.getField("REPLACE").get(null)
            workManagerCls.getMethod(
                "enqueueUniqueWork",
                String::class.java, policyCls,
                Class.forName("androidx.work.OneTimeWorkRequest")
            ).invoke(workManager, identifier, replace, request)
            // Suppress unused warning for `kind` — both kinds map to OneTimeWorkRequest;
            // future work could route processing kind to a `setExpedited` call.
            val _unused = kind
        } catch (e: ClassNotFoundException) {
            Log.w("PerryBackground", "androidx.work not on classpath; schedule() is a no-op")
        } catch (e: Exception) {
            Log.e("PerryBackground", "schedule failed: ${e.message}")
        }
    }

    @JvmStatic
    fun backgroundCancel(identifier: String) {
        try {
            val workManagerCls = Class.forName("androidx.work.WorkManager")
            val workManager = workManagerCls.getMethod("getInstance", Context::class.java)
                .invoke(null, activity)
            workManagerCls.getMethod("cancelUniqueWork", String::class.java)
                .invoke(workManager, identifier)
        } catch (_: ClassNotFoundException) {
        } catch (e: Exception) {
            Log.e("PerryBackground", "cancel failed: ${e.message}")
        }
    }

    // --- Audio Permission ---

    @JvmStatic
    fun requestAudioPermission() {
        // Must run on UI thread — requestPermissions shows a system dialog
        if (Looper.myLooper() == Looper.getMainLooper()) {
            requestAudioPermissionImpl()
        } else {
            uiHandler.post { requestAudioPermissionImpl() }
        }
    }

    private fun requestAudioPermissionImpl() {
        if (ContextCompat.checkSelfPermission(activity, Manifest.permission.RECORD_AUDIO)
            == PackageManager.PERMISSION_GRANTED) {
            audioPermissionGranted = true
        } else {
            ActivityCompat.requestPermissions(
                activity,
                arrayOf(Manifest.permission.RECORD_AUDIO),
                AUDIO_PERMISSION_REQUEST
            )
        }
    }

    fun onAudioPermissionResult(granted: Boolean) {
        audioPermissionGranted = granted
    }

    // --- Camera ---

    @JvmStatic
    fun startCamera(textureView: TextureView) {
        cameraTextureView = textureView
        cameraFrozen = false

        // Start camera background thread
        cameraThread = HandlerThread("PerryCameraThread").also { it.start() }
        cameraHandler = Handler(cameraThread!!.looper)

        if (textureView.isAvailable) {
            openCamera(textureView.surfaceTexture!!)
        } else {
            textureView.surfaceTextureListener = object : TextureView.SurfaceTextureListener {
                override fun onSurfaceTextureAvailable(surface: SurfaceTexture, width: Int, height: Int) {
                    openCamera(surface)
                }
                override fun onSurfaceTextureSizeChanged(surface: SurfaceTexture, width: Int, height: Int) {}
                override fun onSurfaceTextureDestroyed(surface: SurfaceTexture): Boolean = true
                override fun onSurfaceTextureUpdated(surface: SurfaceTexture) {
                    if (!cameraFrozen) {
                        // Capture bitmap from TextureView using PixelCopy for reliable content
                        try {
                            val w = textureView.width
                            val h = textureView.height
                            if (w > 0 && h > 0) {
                                val bmp = Bitmap.createBitmap(w, h, Bitmap.Config.ARGB_8888)
                                val handler = cameraHandler
                                if (handler != null) {
                                    PixelCopy.request(
                                        textureView.surfaceTexture?.let { Surface(it) } ?: return,
                                        bmp,
                                        PixelCopy.OnPixelCopyFinishedListener { result ->
                                            if (result == PixelCopy.SUCCESS) {
                                                latestBitmap = bmp
                                            }
                                        }, handler)
                                }
                            }
                        } catch (_: Exception) {}
                    }
                }
            }
        }
    }

    private fun openCamera(surfaceTexture: SurfaceTexture) {
        val cameraManager = activity.getSystemService(Context.CAMERA_SERVICE) as CameraManager

        // Check permission
        if (ContextCompat.checkSelfPermission(activity, Manifest.permission.CAMERA)
            != PackageManager.PERMISSION_GRANTED) {
            Log.w(TAG, "Camera permission not granted")
            return
        }

        try {
            // Find back-facing camera
            var cameraId: String? = null
            for (id in cameraManager.cameraIdList) {
                val characteristics = cameraManager.getCameraCharacteristics(id)
                val facing = characteristics.get(CameraCharacteristics.LENS_FACING)
                if (facing == CameraCharacteristics.LENS_FACING_BACK) {
                    cameraId = id
                    break
                }
            }
            if (cameraId == null) {
                // Fallback to first camera
                cameraId = cameraManager.cameraIdList.firstOrNull()
            }
            if (cameraId == null) {
                Log.w(TAG, "No camera found")
                return
            }

            // Configure transform matrix for proper aspect ratio (center-crop)
            val characteristics = cameraManager.getCameraCharacteristics(cameraId)
            val map = characteristics.get(CameraCharacteristics.SCALER_STREAM_CONFIGURATION_MAP)
            val previewSize = map?.getOutputSizes(SurfaceTexture::class.java)
                ?.maxByOrNull { it.width * it.height } ?: android.util.Size(1920, 1080)
            surfaceTexture.setDefaultBufferSize(previewSize.width, previewSize.height)

            val textureView = cameraTextureView
            if (textureView != null && textureView.width > 0 && textureView.height > 0) {
                val viewWidth = textureView.width.toFloat()
                val viewHeight = textureView.height.toFloat()
                val previewWidth = previewSize.height.toFloat()  // rotated 90°
                val previewHeight = previewSize.width.toFloat()
                val scaleX = viewWidth / previewWidth
                val scaleY = viewHeight / previewHeight
                val scale = Math.max(scaleX, scaleY)  // center-crop (fill)
                val matrix = android.graphics.Matrix()
                matrix.setScale(
                    scale * previewWidth / viewWidth,
                    scale * previewHeight / viewHeight,
                    viewWidth / 2f, viewHeight / 2f
                )
                textureView.setTransform(matrix)
            }

            cameraManager.openCamera(cameraId, object : CameraDevice.StateCallback() {
                override fun onOpened(camera: CameraDevice) {
                    cameraDevice = camera
                    createCaptureSession(camera, surfaceTexture)
                }
                override fun onDisconnected(camera: CameraDevice) {
                    camera.close()
                    cameraDevice = null
                }
                override fun onError(camera: CameraDevice, error: Int) {
                    Log.e(TAG, "Camera error: $error")
                    camera.close()
                    cameraDevice = null
                }
            }, cameraHandler)
        } catch (e: CameraAccessException) {
            Log.e(TAG, "Failed to open camera", e)
        }
    }

    private fun createCaptureSession(camera: CameraDevice, surfaceTexture: SurfaceTexture) {
        try {
            // Configure preview surface from TextureView
            val previewSurface = Surface(surfaceTexture)

            // Create ImageReader for color sampling (small resolution is enough)
            val reader = ImageReader.newInstance(640, 480, ImageFormat.YUV_420_888, 2)
            imageReader = reader
            reader.setOnImageAvailableListener({ ir ->
                val image = ir.acquireLatestImage() ?: return@setOnImageAvailableListener
                try {
                    if (!cameraFrozen) {
                        // Store YUV data for on-demand sampling (avoid full-frame conversion)
                        val w = image.width
                        val h = image.height
                        val yPlane = image.planes[0]
                        val uPlane = image.planes[1]
                        val vPlane = image.planes[2]

                        // Copy plane data (image is recycled after close)
                        val yBuf = yPlane.buffer
                        val uBuf = uPlane.buffer
                        val vBuf = vPlane.buffer
                        val yBytes = ByteArray(yBuf.remaining()); yBuf.get(yBytes)
                        val uBytes = ByteArray(uBuf.remaining()); uBuf.get(uBytes)
                        val vBytes = ByteArray(vBuf.remaining()); vBuf.get(vBytes)

                        latestYuvFrame = YuvFrame(w, h, yBytes, uBytes, vBytes,
                            yPlane.rowStride, uPlane.rowStride, uPlane.pixelStride)
                    }
                } finally {
                    image.close()
                }
            }, cameraHandler)

            val captureRequestBuilder = camera.createCaptureRequest(CameraDevice.TEMPLATE_PREVIEW)
            captureRequestBuilder.addTarget(previewSurface)
            captureRequestBuilder.addTarget(reader.surface)

            // Auto-focus
            captureRequestBuilder.set(
                CaptureRequest.CONTROL_AF_MODE,
                CaptureRequest.CONTROL_AF_MODE_CONTINUOUS_PICTURE
            )

            camera.createCaptureSession(
                listOf(previewSurface, reader.surface),
                object : CameraCaptureSession.StateCallback() {
                    override fun onConfigured(session: CameraCaptureSession) {
                        captureSession = session
                        try {
                            session.setRepeatingRequest(
                                captureRequestBuilder.build(),
                                null,
                                cameraHandler
                            )
                            Log.d(TAG, "Camera preview started")
                        } catch (e: CameraAccessException) {
                            Log.e(TAG, "Failed to start preview", e)
                        }
                    }
                    override fun onConfigureFailed(session: CameraCaptureSession) {
                        Log.e(TAG, "Camera session configuration failed")
                    }
                },
                cameraHandler
            )
        } catch (e: CameraAccessException) {
            Log.e(TAG, "Failed to create capture session", e)
        }
    }

    @JvmStatic
    fun stopCamera() {
        captureSession?.close()
        captureSession = null
        cameraDevice?.close()
        cameraDevice = null
        imageReader?.close()
        imageReader = null
        cameraThread?.quitSafely()
        cameraThread = null
        cameraHandler = null
        latestBitmap = null
        cameraFrozen = false
        Log.d(TAG, "Camera stopped")
    }

    @JvmStatic
    fun freezeCamera() {
        cameraFrozen = true
        // Stop the repeating request to freeze the preview
        try {
            captureSession?.stopRepeating()
        } catch (_: Exception) {}
        Log.d(TAG, "Camera frozen")
    }

    @JvmStatic
    fun unfreezeCamera() {
        cameraFrozen = false
        // Restart repeating request
        val session = captureSession ?: return
        val device = cameraDevice ?: return
        val textureView = cameraTextureView ?: return
        val surfaceTexture = textureView.surfaceTexture ?: return
        try {
            val previewSurface = Surface(surfaceTexture)
            val builder = device.createCaptureRequest(CameraDevice.TEMPLATE_PREVIEW)
            builder.addTarget(previewSurface)
            builder.set(
                CaptureRequest.CONTROL_AF_MODE,
                CaptureRequest.CONTROL_AF_MODE_CONTINUOUS_PICTURE
            )
            session.setRepeatingRequest(builder.build(), null, cameraHandler)
            Log.d(TAG, "Camera unfrozen")
        } catch (e: Exception) {
            Log.e(TAG, "Failed to unfreeze camera", e)
        }
    }

    /**
     * Sample the color at normalized (0-1) coordinates from the latest camera frame.
     * Returns packed RGB: r * 65536 + g * 256 + b, or -1.0 if unavailable.
     */
    @JvmStatic
    fun cameraSampleColor(x: Double, y: Double): Double {
        val frame = latestYuvFrame ?: return -1.0
        val w = frame.width
        val h = frame.height
        if (w == 0 || h == 0) return -1.0

        // YUV frame is landscape (from sensor). Remap portrait screen coords.
        val normX: Double
        val normY: Double
        if (w > h) {
            normX = (1.0 - y).coerceIn(0.0, 1.0)
            normY = x.coerceIn(0.0, 1.0)
        } else {
            normX = x.coerceIn(0.0, 1.0)
            normY = y.coerceIn(0.0, 1.0)
        }

        val (r, g, b) = frame.sampleRgb(normX, normY)
        return r * 65536.0 + g * 256.0 + b
    }

    /**
     * Set a tap handler on a camera view that reports normalized (x, y) coordinates.
     * Uses the callback2 mechanism to pass (normX, normY) back to Rust.
     */
    @JvmStatic
    fun setCameraTapCallback(view: View, callbackKey: Long) {
        view.setOnTouchListener { v, event ->
            if (event.action == MotionEvent.ACTION_UP) {
                val normX = if (v.width > 0) (event.x / v.width).toDouble() else 0.5
                val normY = if (v.height > 0) (event.y / v.height).toDouble() else 0.5
                nativeInvokeCallback2(callbackKey, normX, normY)
            }
            true
        }
    }

    // --- Toast (Phase 2 v3.3) ---

    /**
     * Show an Android Toast message on the UI thread.
     * Called from the Perry native thread via JNI; posts to uiHandler so
     * Toast.makeText runs on the main looper as required by the Android SDK.
     * Toast.LENGTH_SHORT = 0 (approx 2s), consistent with the macOS 2.5s hold.
     */
    @JvmStatic
    fun showToast(msg: String) {
        val ctx = activity
        uiHandler.post {
            Toast.makeText(ctx, msg, Toast.LENGTH_SHORT).show()
        }
    }

    // --- Timer ---

    @JvmStatic
    fun setTimer(callbackKey: Long, intervalMs: Long) {
        val runnable = object : Runnable {
            override fun run() {
                nativeInvokeCallback0(callbackKey)
                uiHandler.postDelayed(this, intervalMs)
            }
        }
        uiHandler.postDelayed(runnable, intervalMs)
    }

    // --- Timer pump (equivalent to iOS PerryPumpTarget 8ms NSTimer) ---

    /**
     * Start the runtime pump timer that drives setInterval/setTimeout/Promise
     * callbacks. Fires every [intervalMs] milliseconds and calls nativePumpTick().
     * Without this, the Perry runtime timers never fire on Android.
     */
    @JvmStatic
    fun startPumpTimer(intervalMs: Long) {
        val pumpRunnable = object : Runnable {
            override fun run() {
                nativePumpTick()
                uiHandler.postDelayed(this, intervalMs)
            }
        }
        uiHandler.postDelayed(pumpRunnable, intervalMs)
    }

    // --- EditText submit callback (Enter/Done key) ---

    @JvmStatic
    fun setOnSubmitCallback(editText: EditText, callbackKey: Long) {
        editText.setOnEditorActionListener { _, actionId, _ ->
            // IME_ACTION_DONE=6, IME_ACTION_GO=2, IME_ACTION_SEND=4, IME_ACTION_SEARCH=3
            if (actionId == android.view.inputmethod.EditorInfo.IME_ACTION_DONE ||
                actionId == android.view.inputmethod.EditorInfo.IME_ACTION_GO ||
                actionId == android.view.inputmethod.EditorInfo.IME_ACTION_SEND ||
                actionId == android.view.inputmethod.EditorInfo.IME_ACTION_SEARCH) {
                nativeInvokeCallback0(callbackKey)
                true
            } else {
                false
            }
        }
    }

    // --- Native methods ---

    @JvmStatic
    external fun nativeInit()

    @JvmStatic
    external fun nativeMain()

    @JvmStatic
    external fun nativeShutdown()

    @JvmStatic
    external fun nativePumpTick()

    // #6184 (2026-07-09 GC audit): OS memory-pressure → GC bridge.
    // PerryActivity.onLowMemory() / onTrimMemory(level) forward here. Level
    // 1 = minor (collect-if-safe), level 2 = critical (full collect + clamp
    // the arena trigger).
    @JvmStatic
    external fun nativeMemoryPressure(level: Int)

    @JvmStatic
    external fun nativeInvokeCallback0(key: Long)

    @JvmStatic
    external fun nativeInvokeCallback1(key: Long, arg: Double)

    @JvmStatic
    external fun nativeInvokeCallback2(key: Long, arg1: Double, arg2: Double)

    // Continuous pointer events (issue #1868). Native side allocates a
    // `PointerEvent { x, y, button, pointerType: "touch" }` and invokes
    // the closure registered under `key`.
    external fun nativeInvokePointerCallback(key: Long, x: Double, y: Double, button: Int)

    @JvmStatic
    external fun nativeInvokeCallbackWithString(key: Long, text: String)

    // Issue #582: network reachability — `(connected, kind)` argument pair.
    @JvmStatic
    external fun nativeInvokeNetworkCallback(key: Long, connected: Boolean, kind: String)

    // Issue #583: deep links — `(url, source)` argument pair where source
    // is `"cold-start"` or `"foreground"`.
    @JvmStatic
    external fun nativeInvokeDeepLinkCallback(key: Long, url: String, source: String)

    // Issue #1864: continuous keyboard events. Called from
    // PerryActivity.dispatchKeyEvent on every key transition.
    @JvmStatic
    external fun nativeDispatchKey(
        keyCode: Int,
        action: Int,
        metaState: Int,
        repeatCount: Int,
    )

    // =====================================================================
    // MapView (issue #517) — Google Maps SDK for Android.
    // =====================================================================
    //
    // MapView has its own activity lifecycle: onCreate / onResume / onPause /
    // onLowMemory / onDestroy must be called explicitly. PerryActivity forwards
    // those events via `forwardMapsLifecycle()` below; this object keeps a
    // list of every MapView constructed so the forwarding hits all of them.
    //
    // The GoogleMap object isn't immediately available — it's loaded
    // asynchronously by `getMapAsync`. We park early `setRegion` / `addPin` /
    // `clearPins` / `setMapType` calls in a per-MapView pending-ops queue
    // and replay them once the GoogleMap callback fires.

    private val mapViews = mutableListOf<com.google.android.gms.maps.MapView>()
    private val mapPending = mutableMapOf<com.google.android.gms.maps.MapView,
        MutableList<(com.google.android.gms.maps.GoogleMap) -> Unit>>()
    private val mapReady = mutableMapOf<com.google.android.gms.maps.MapView,
        com.google.android.gms.maps.GoogleMap>()

    /// Approximate the (lat_span, lon_span) → tile-zoom mapping that
    /// Google Maps' camera uses (zoom = log2(360 / span_deg)).
    private fun zoomFromSpan(latSpan: Double, lonSpan: Double): Float {
        val span = Math.max(latSpan.coerceAtLeast(0.0001), lonSpan.coerceAtLeast(0.0001))
        return (Math.log(360.0 / span) / Math.log(2.0)).coerceIn(0.0, 21.0).toFloat()
    }

    @JvmStatic
    fun mapViewCreate(width: Double, height: Double): com.google.android.gms.maps.MapView {
        val mapView = com.google.android.gms.maps.MapView(activity)
        mapView.onCreate(null)
        mapView.onResume()
        // Apply requested layout size (the dispatch table converts widget
        // f64 args to the LinearLayout it gets attached to; we set the
        // initial layoutParams here so first paint isn't 0×0).
        mapView.layoutParams = FrameLayout.LayoutParams(
            width.toInt().coerceAtLeast(80),
            height.toInt().coerceAtLeast(80)
        )
        mapViews.add(mapView)
        mapPending[mapView] = mutableListOf()

        mapView.getMapAsync { gmap ->
            mapReady[mapView] = gmap
            mapPending.remove(mapView)?.forEach { it(gmap) }
        }

        return mapView
    }

    private fun runOnMap(
        mapView: com.google.android.gms.maps.MapView,
        op: (com.google.android.gms.maps.GoogleMap) -> Unit
    ) {
        val ready = mapReady[mapView]
        if (ready != null) {
            uiHandler.post { op(ready) }
        } else {
            mapPending.getOrPut(mapView) { mutableListOf() }.add(op)
        }
    }

    @JvmStatic
    fun mapViewSetRegion(
        mapView: com.google.android.gms.maps.MapView,
        latitude: Double,
        longitude: Double,
        latSpan: Double,
        lonSpan: Double
    ) {
        val zoom = zoomFromSpan(latSpan, lonSpan)
        runOnMap(mapView) { gmap ->
            val pos = com.google.android.gms.maps.model.CameraPosition.Builder()
                .target(com.google.android.gms.maps.model.LatLng(latitude, longitude))
                .zoom(zoom)
                .build()
            gmap.animateCamera(
                com.google.android.gms.maps.CameraUpdateFactory.newCameraPosition(pos)
            )
        }
    }

    @JvmStatic
    fun mapViewAddPin(
        mapView: com.google.android.gms.maps.MapView,
        latitude: Double,
        longitude: Double,
        title: String
    ) {
        runOnMap(mapView) { gmap ->
            val opts = com.google.android.gms.maps.model.MarkerOptions()
                .position(com.google.android.gms.maps.model.LatLng(latitude, longitude))
            if (title.isNotEmpty()) {
                opts.title(title)
            }
            gmap.addMarker(opts)
        }
    }

    @JvmStatic
    fun mapViewClearPins(mapView: com.google.android.gms.maps.MapView) {
        runOnMap(mapView) { gmap -> gmap.clear() }
    }

    @JvmStatic
    fun mapViewSetMapType(mapView: com.google.android.gms.maps.MapView, style: Long) {
        // Match MapKit's MKMapType enum order: 0=standard, 1=satellite,
        // 2=hybrid. Google Maps uses MAP_TYPE_NORMAL=1, _SATELLITE=2,
        // _TERRAIN=3, _HYBRID=4 — translate explicitly.
        val gmapType = when (style.toInt()) {
            1 -> com.google.android.gms.maps.GoogleMap.MAP_TYPE_SATELLITE
            2 -> com.google.android.gms.maps.GoogleMap.MAP_TYPE_HYBRID
            else -> com.google.android.gms.maps.GoogleMap.MAP_TYPE_NORMAL
        }
        runOnMap(mapView) { gmap -> gmap.mapType = gmapType }
    }

    /// Called by PerryActivity for each lifecycle event. `event` ∈
    /// `"resume" | "pause" | "destroy" | "lowMemory"`.
    @JvmStatic
    fun forwardMapsLifecycle(event: String) {
        when (event) {
            "resume" -> mapViews.forEach { it.onResume() }
            "pause" -> mapViews.forEach { it.onPause() }
            "lowMemory" -> mapViews.forEach { it.onLowMemory() }
            "destroy" -> {
                mapViews.forEach { it.onDestroy() }
                mapViews.clear()
                mapPending.clear()
                mapReady.clear()
            }
        }
    }

    // ============================================================
    // Issue #481 — Calendar widget (android.widget.CalendarView).
    // ============================================================
    //
    // CalendarView's `setOnDateChangeListener` fires with (year, monthZeroBased,
    // day); we format `yyyy-MM-dd` (POSIX/ISO) here so the cross-platform
    // string matches the macOS / gtk4 / iOS twins exactly.

    @JvmStatic
    fun calendarCreate(year: Long, month: Long, callbackKey: Long): android.widget.CalendarView {
        val cv = android.widget.CalendarView(activity)
        if (year > 0L && month in 1L..12L) {
            val cal = java.util.Calendar.getInstance()
            cal.set(year.toInt(), (month - 1).toInt(), 1)
            cv.date = cal.timeInMillis
        }
        if (callbackKey != 0L) {
            cv.setOnDateChangeListener { _, y, m, d ->
                val iso = String.format("%04d-%02d-%02d", y, m + 1, d)
                nativeInvokeCallbackWithString(callbackKey, iso)
            }
        }
        return cv
    }

    @JvmStatic
    fun calendarSetDate(cv: android.widget.CalendarView, year: Long, month: Long, day: Long) {
        if (year <= 0L || month !in 1L..12L || day !in 1L..31L) return
        val cal = java.util.Calendar.getInstance()
        cal.set(year.toInt(), (month - 1).toInt(), day.toInt())
        uiHandler.post {
            cv.date = cal.timeInMillis
        }
    }

    @JvmStatic
    fun calendarGetSelectedDate(cv: android.widget.CalendarView): String {
        val cal = java.util.Calendar.getInstance()
        cal.timeInMillis = cv.date
        val y = cal.get(java.util.Calendar.YEAR)
        val m = cal.get(java.util.Calendar.MONTH) + 1
        val d = cal.get(java.util.Calendar.DAY_OF_MONTH)
        return String.format("%04d-%02d-%02d", y, m, d)
    }

    // ============================================================
    // Issue #4772 — DatePicker (android.widget.DatePicker).
    // ============================================================
    //
    // The compact, spinner-style complement to the CalendarView-backed
    // Calendar widget. `DatePicker.init` installs an OnDateChangedListener
    // that fires with (year, monthZeroBased, day); we format `yyyy-MM-dd`
    // (POSIX/ISO) here so the cross-platform string matches the macOS /
    // iOS / gtk4 / Windows twins exactly.

    @JvmStatic
    fun datePickerCreate(year: Long, month: Long, callbackKey: Long): android.widget.DatePicker {
        val dp = android.widget.DatePicker(activity)
        val cal = java.util.Calendar.getInstance()
        val initYear = if (year > 0L) year.toInt() else cal.get(java.util.Calendar.YEAR)
        val initMonth = if (month in 1L..12L) (month - 1).toInt() else cal.get(java.util.Calendar.MONTH)
        dp.init(initYear, initMonth, 1) { _, y, m, d ->
            if (callbackKey != 0L) {
                val iso = String.format("%04d-%02d-%02d", y, m + 1, d)
                nativeInvokeCallbackWithString(callbackKey, iso)
            }
        }
        return dp
    }

    @JvmStatic
    fun datePickerSetDate(dp: android.widget.DatePicker, year: Long, month: Long, day: Long) {
        if (year <= 0L || month !in 1L..12L || day !in 1L..31L) return
        uiHandler.post {
            dp.updateDate(year.toInt(), (month - 1).toInt(), day.toInt())
        }
    }

    @JvmStatic
    fun datePickerGetSelectedDate(dp: android.widget.DatePicker): String {
        val y = dp.year
        val m = dp.month + 1
        val d = dp.dayOfMonth
        return String.format("%04d-%02d-%02d", y, m, d)
    }

    // ============================================================
    // Issue #475 — Combobox (android.widget.AutoCompleteTextView).
    // ============================================================
    //
    // Each combobox keeps its own ArrayAdapter<String> in a side-map so
    // `comboboxAddItem` only needs to push one entry instead of rebuilding
    // the whole list. The TextWatcher fires nativeInvokeCallbackWithString
    // on each edit; selecting an autocomplete suggestion already feeds the
    // text back through the same path.

    private val comboboxAdapters =
        mutableMapOf<android.widget.AutoCompleteTextView, ArrayAdapter<String>>()

    @JvmStatic
    fun comboboxCreate(
        initial: String,
        callbackKey: Long
    ): android.widget.AutoCompleteTextView {
        val actv = android.widget.AutoCompleteTextView(activity)
        val adapter = ArrayAdapter<String>(
            activity,
            android.R.layout.simple_dropdown_item_1line,
            mutableListOf<String>()
        )
        actv.setAdapter(adapter)
        actv.threshold = 1
        if (initial.isNotEmpty()) {
            actv.setText(initial, false)
        }
        comboboxAdapters[actv] = adapter
        if (callbackKey != 0L) {
            actv.addTextChangedListener(object : TextWatcher {
                override fun beforeTextChanged(s: CharSequence?, start: Int, count: Int, after: Int) {}
                override fun onTextChanged(s: CharSequence?, start: Int, before: Int, count: Int) {}
                override fun afterTextChanged(s: Editable?) {
                    nativeInvokeCallbackWithString(callbackKey, s?.toString() ?: "")
                }
            })
        }
        return actv
    }

    @JvmStatic
    fun comboboxAddItem(actv: android.widget.AutoCompleteTextView, value: String) {
        val adapter = comboboxAdapters[actv] ?: return
        uiHandler.post {
            adapter.add(value)
            adapter.notifyDataSetChanged()
        }
    }

    @JvmStatic
    fun comboboxSetValue(actv: android.widget.AutoCompleteTextView, value: String) {
        uiHandler.post {
            actv.setText(value, false)
        }
    }

    @JvmStatic
    fun comboboxGetValue(actv: android.widget.AutoCompleteTextView): String {
        return actv.text?.toString() ?: ""
    }

    // ============================================================
    // Issue #478 — RichText editor (EditText + SpannableStringBuilder).
    // ============================================================
    //
    // EditText stores its content as a SpannableStringBuilder under the
    // hood, so every span we apply via toggleBold/toggleItalic/etc.
    // survives the round-trip back through `Html.toHtml`. Toggle methods
    // apply the requested style to the active selection (or insertion
    // point) — matching the macOS NSTextView twin's behavior.

    @JvmStatic
    fun richTextCreate(width: Double, height: Double, callbackKey: Long): EditText {
        val et = EditText(activity)
        et.gravity = android.view.Gravity.TOP or android.view.Gravity.START
        et.setSingleLine(false)
        et.isVerticalScrollBarEnabled = true
        if (width > 0 || height > 0) {
            et.layoutParams = FrameLayout.LayoutParams(
                if (width > 0) width.toInt() else FrameLayout.LayoutParams.MATCH_PARENT,
                if (height > 0) height.toInt() else FrameLayout.LayoutParams.WRAP_CONTENT
            )
        }
        if (callbackKey != 0L) {
            et.addTextChangedListener(object : TextWatcher {
                override fun beforeTextChanged(s: CharSequence?, start: Int, count: Int, after: Int) {}
                override fun onTextChanged(s: CharSequence?, start: Int, before: Int, count: Int) {}
                override fun afterTextChanged(s: Editable?) {
                    nativeInvokeCallbackWithString(callbackKey, s?.toString() ?: "")
                }
            })
        }
        return et
    }

    @JvmStatic
    fun richTextSetString(et: EditText, text: String) {
        uiHandler.post { et.setText(text) }
    }

    @JvmStatic
    fun richTextGetString(et: EditText): String {
        return et.text?.toString() ?: ""
    }

    @JvmStatic
    fun richTextSetHtml(et: EditText, html: String) {
        uiHandler.post {
            val spanned: android.text.Spanned = if (android.os.Build.VERSION.SDK_INT >= 24) {
                android.text.Html.fromHtml(html, android.text.Html.FROM_HTML_MODE_COMPACT)
            } else {
                @Suppress("DEPRECATION")
                android.text.Html.fromHtml(html)
            }
            et.setText(spanned)
        }
    }

    @JvmStatic
    fun richTextGetHtml(et: EditText): String {
        val text = et.text ?: return ""
        val spanned: android.text.Spanned = if (text is android.text.Spanned) {
            text
        } else {
            android.text.SpannableStringBuilder(text)
        }
        return if (android.os.Build.VERSION.SDK_INT >= 24) {
            android.text.Html.toHtml(
                spanned,
                android.text.Html.TO_HTML_PARAGRAPH_LINES_CONSECUTIVE
            )
        } else {
            @Suppress("DEPRECATION")
            android.text.Html.toHtml(spanned)
        }
    }

    private fun richTextSelectionRange(et: EditText): IntArray {
        val start = et.selectionStart.coerceAtLeast(0)
        val end = et.selectionEnd.coerceAtLeast(start)
        if (start == end) {
            return intArrayOf(0, et.text?.length ?: 0)
        }
        return intArrayOf(start, end)
    }

    private inline fun <reified T : android.text.style.CharacterStyle> richTextToggleSpan(
        et: EditText,
        crossinline factory: () -> T,
        crossinline matches: (T) -> Boolean,
    ) {
        uiHandler.post {
            val editable = et.text ?: return@post
            val (start, end) = richTextSelectionRange(et).let { it[0] to it[1] }
            if (start >= end) return@post
            val existing = editable.getSpans(start, end, T::class.java).filter(matches)
            if (existing.isNotEmpty()) {
                for (sp in existing) {
                    editable.removeSpan(sp)
                }
            } else {
                editable.setSpan(
                    factory(),
                    start,
                    end,
                    android.text.Spannable.SPAN_EXCLUSIVE_EXCLUSIVE
                )
            }
        }
    }

    @JvmStatic
    fun richTextToggleBold(et: EditText) {
        richTextToggleSpan(
            et,
            { android.text.style.StyleSpan(android.graphics.Typeface.BOLD) },
            { it.style == android.graphics.Typeface.BOLD || it.style == android.graphics.Typeface.BOLD_ITALIC }
        )
    }

    @JvmStatic
    fun richTextToggleItalic(et: EditText) {
        richTextToggleSpan(
            et,
            { android.text.style.StyleSpan(android.graphics.Typeface.ITALIC) },
            { it.style == android.graphics.Typeface.ITALIC || it.style == android.graphics.Typeface.BOLD_ITALIC }
        )
    }

    // ============================================================
    // Issue #474 — Chart widget (custom View.onDraw, 3 chart kinds).
    // ============================================================
    //
    // Kotlin owns the data; Rust just pushes points and asks for redraws
    // (`postInvalidate`). kind: 0=line, 1=bar, 2=pie. PerryChartView keeps
    // its own MutableList<Pair<String,Double>> and renders directly in
    // onDraw — no graphing library dep.

    private class PerryChartView(
        context: Context,
        var kind: Int
    ) : View(context) {
        val points: MutableList<Pair<String, Double>> = mutableListOf()
        var title: String = ""

        private val barFillPaint = android.graphics.Paint(android.graphics.Paint.ANTI_ALIAS_FLAG).apply {
            color = 0xFF4287F5.toInt()
            style = android.graphics.Paint.Style.FILL
        }
        private val linePaint = android.graphics.Paint(android.graphics.Paint.ANTI_ALIAS_FLAG).apply {
            color = 0xFF4287F5.toInt()
            style = android.graphics.Paint.Style.STROKE
            strokeWidth = 4f
        }
        private val pointPaint = android.graphics.Paint(android.graphics.Paint.ANTI_ALIAS_FLAG).apply {
            color = 0xFF1B5FB8.toInt()
            style = android.graphics.Paint.Style.FILL
        }
        private val axisPaint = android.graphics.Paint(android.graphics.Paint.ANTI_ALIAS_FLAG).apply {
            color = 0xFF888888.toInt()
            style = android.graphics.Paint.Style.STROKE
            strokeWidth = 2f
        }
        private val labelPaint = android.graphics.Paint(android.graphics.Paint.ANTI_ALIAS_FLAG).apply {
            color = 0xFF222222.toInt()
            textSize = 24f
            textAlign = android.graphics.Paint.Align.CENTER
        }
        private val titlePaint = android.graphics.Paint(android.graphics.Paint.ANTI_ALIAS_FLAG).apply {
            color = 0xFF222222.toInt()
            textSize = 32f
            textAlign = android.graphics.Paint.Align.CENTER
            isFakeBoldText = true
        }
        private val pieColors = intArrayOf(
            0xFF4287F5.toInt(), 0xFFE54545.toInt(), 0xFF34A853.toInt(),
            0xFFFBBC05.toInt(), 0xFF9C27B0.toInt(), 0xFF00ACC1.toInt(),
            0xFFFF7043.toInt(), 0xFF8D6E63.toInt()
        )

        override fun onDraw(canvas: android.graphics.Canvas) {
            super.onDraw(canvas)
            val w = width.toFloat()
            val h = height.toFloat()
            val titleHeight = if (title.isNotEmpty()) 48f else 0f
            if (title.isNotEmpty()) {
                canvas.drawText(title, w / 2f, 36f, titlePaint)
            }
            if (points.isEmpty()) return

            when (kind) {
                0 -> drawLine(canvas, w, h, titleHeight)
                1 -> drawBar(canvas, w, h, titleHeight)
                2 -> drawPie(canvas, w, h, titleHeight)
                else -> drawBar(canvas, w, h, titleHeight)
            }
        }

        private fun drawBar(canvas: android.graphics.Canvas, w: Float, h: Float, top: Float) {
            val padL = 48f; val padR = 16f; val padB = 48f; val padT = top + 16f
            val plotW = w - padL - padR
            val plotH = h - padT - padB
            if (plotW <= 0 || plotH <= 0) return
            val maxV = points.maxOf { it.second }.coerceAtLeast(1e-9)
            val barWidth = plotW / points.size * 0.7f
            val step = plotW / points.size
            canvas.drawLine(padL, h - padB, w - padR, h - padB, axisPaint)
            for ((i, pt) in points.withIndex()) {
                val cx = padL + step * (i + 0.5f)
                val barH = (pt.second / maxV * plotH).toFloat()
                val left = cx - barWidth / 2f
                val top0 = (h - padB) - barH
                canvas.drawRect(left, top0, left + barWidth, h - padB, barFillPaint)
                canvas.drawText(pt.first, cx, h - padB + 28f, labelPaint)
            }
        }

        private fun drawLine(canvas: android.graphics.Canvas, w: Float, h: Float, top: Float) {
            val padL = 48f; val padR = 16f; val padB = 48f; val padT = top + 16f
            val plotW = w - padL - padR
            val plotH = h - padT - padB
            if (plotW <= 0 || plotH <= 0) return
            val maxV = points.maxOf { it.second }.coerceAtLeast(1e-9)
            val step = if (points.size <= 1) 0f else plotW / (points.size - 1)
            canvas.drawLine(padL, h - padB, w - padR, h - padB, axisPaint)
            val path = android.graphics.Path()
            for ((i, pt) in points.withIndex()) {
                val cx = padL + step * i
                val cy = (h - padB) - (pt.second / maxV * plotH).toFloat()
                if (i == 0) path.moveTo(cx, cy) else path.lineTo(cx, cy)
            }
            canvas.drawPath(path, linePaint)
            for ((i, pt) in points.withIndex()) {
                val cx = padL + step * i
                val cy = (h - padB) - (pt.second / maxV * plotH).toFloat()
                canvas.drawCircle(cx, cy, 6f, pointPaint)
                canvas.drawText(pt.first, cx, h - padB + 28f, labelPaint)
            }
        }

        private fun drawPie(canvas: android.graphics.Canvas, w: Float, h: Float, top: Float) {
            val sum = points.sumOf { it.second }
            if (sum <= 0) return
            val padT = top + 16f
            val plotW = w
            val plotH = h - padT - 16f
            val r = (Math.min(plotW, plotH) / 2f - 16f).coerceAtLeast(8f)
            val cx = w / 2f
            val cy = padT + plotH / 2f
            val rect = android.graphics.RectF(cx - r, cy - r, cx + r, cy + r)
            val slicePaint = android.graphics.Paint(android.graphics.Paint.ANTI_ALIAS_FLAG).apply {
                style = android.graphics.Paint.Style.FILL
            }
            var startAngle = -90f
            for ((i, pt) in points.withIndex()) {
                val sweep = (pt.second / sum * 360.0).toFloat()
                slicePaint.color = pieColors[i % pieColors.size]
                canvas.drawArc(rect, startAngle, sweep, true, slicePaint)
                startAngle += sweep
            }
            var ly = padT + 16f
            for ((i, pt) in points.withIndex()) {
                slicePaint.color = pieColors[i % pieColors.size]
                canvas.drawRect(16f, ly - 16f, 36f, ly, slicePaint)
                canvas.drawText(pt.first, 80f, ly, labelPaint.apply {
                    textAlign = android.graphics.Paint.Align.LEFT
                })
                ly += 28f
            }
            labelPaint.textAlign = android.graphics.Paint.Align.CENTER
        }
    }

    @JvmStatic
    fun chartCreate(kind: Long, width: Double, height: Double): View {
        val v = PerryChartView(activity, kind.toInt())
        if (width > 0 || height > 0) {
            v.layoutParams = FrameLayout.LayoutParams(
                if (width > 0) width.toInt() else FrameLayout.LayoutParams.MATCH_PARENT,
                if (height > 0) height.toInt() else FrameLayout.LayoutParams.WRAP_CONTENT
            )
        }
        return v
    }

    @JvmStatic
    fun chartAddDataPoint(v: View, label: String, value: Double) {
        val chart = v as? PerryChartView ?: return
        uiHandler.post {
            chart.points.add(label to value)
            chart.invalidate()
        }
    }

    @JvmStatic
    fun chartClearData(v: View) {
        val chart = v as? PerryChartView ?: return
        uiHandler.post {
            chart.points.clear()
            chart.invalidate()
        }
    }

    @JvmStatic
    fun chartSetTitle(v: View, title: String) {
        val chart = v as? PerryChartView ?: return
        uiHandler.post {
            chart.title = title
            chart.invalidate()
        }
    }

    // ============================================================
    // Issue #480 — TreeView (flat ListView with depth-aware adapter).
    // ============================================================
    //
    // Rust owns the node graph; Kotlin only renders. `treeViewRefresh`
    // receives a pre-flattened (rows, ids) pair where `rows` contain the
    // already-indented strings ("    ▾ Label") and `ids` are the parallel
    // node IDs. OnItemClickListener forwards the tapped id back to Rust
    // via `nativeTreeRowTapped(widgetHandle, id)`, which both toggles
    // expand/collapse state and fires the user on_select closure.

    private val treeViewAdapters = mutableMapOf<android.widget.ListView, ArrayAdapter<String>>()
    private val treeViewIds = mutableMapOf<android.widget.ListView, Array<String>>()

    @JvmStatic
    fun treeViewCreate(callbackKey: Long): android.widget.ListView {
        val lv = android.widget.ListView(activity)
        val adapter = ArrayAdapter<String>(
            activity,
            android.R.layout.simple_list_item_1,
            mutableListOf<String>()
        )
        lv.adapter = adapter
        treeViewAdapters[lv] = adapter
        lv.setOnItemClickListener { _, _, position, _ ->
            val ids = treeViewIds[lv] ?: return@setOnItemClickListener
            if (position in ids.indices) {
                val handle = (lv.tag as? Long) ?: 0L
                nativeTreeRowTapped(handle, ids[position])
            }
        }
        return lv
    }

    @JvmStatic
    fun treeViewRefresh(
        widgetHandle: Long,
        lv: android.widget.ListView,
        rows: Array<String>,
        ids: Array<String>
    ) {
        uiHandler.post {
            lv.tag = widgetHandle
            treeViewIds[lv] = ids
            val adapter = treeViewAdapters[lv] ?: return@post
            adapter.clear()
            adapter.addAll(rows.toList())
            adapter.notifyDataSetChanged()
        }
    }

    // ============================================================
    // Issue #479 — RichTooltip (long-press PopupWindow).
    // ============================================================
    //
    // Android has no hover model on touch devices, so the cross-platform
    // `hover_delay_ms` argument is dropped on the Rust side and the system
    // long-press duration (~500 ms) is used here. The popup hosts the
    // content widget subtree directly; outside-touch dismisses it.

    @JvmStatic
    fun setRichTooltip(trigger: View, content: View) {
        trigger.setOnLongClickListener {
            (content.parent as? android.view.ViewGroup)?.removeView(content)
            val popup = android.widget.PopupWindow(
                content,
                android.view.ViewGroup.LayoutParams.WRAP_CONTENT,
                android.view.ViewGroup.LayoutParams.WRAP_CONTENT,
                true
            )
            popup.isOutsideTouchable = true
            popup.setBackgroundDrawable(
                android.graphics.drawable.ColorDrawable(0xFFFFFFFF.toInt())
            )
            popup.elevation = 8f
            popup.showAsDropDown(trigger)
            true
        }
    }

    @JvmStatic
    fun richTextToggleUnderline(et: EditText) {
        uiHandler.post {
            val editable = et.text ?: return@post
            val (start, end) = richTextSelectionRange(et).let { it[0] to it[1] }
            if (start >= end) return@post
            val existing = editable.getSpans(start, end, android.text.style.UnderlineSpan::class.java)
            if (existing.isNotEmpty()) {
                for (sp in existing) editable.removeSpan(sp)
            } else {
                editable.setSpan(
                    android.text.style.UnderlineSpan(),
                    start,
                    end,
                    android.text.Spannable.SPAN_EXCLUSIVE_EXCLUSIVE
                )
            }
        }
    }

    // Issue #552 — extra invoke shapes for the geolocation success callback
    // (4 doubles) and the image-picker callback (string array).
    @JvmStatic
    external fun nativeInvokeCallback4(key: Long, a0: Double, a1: Double, a2: Double, a3: Double)

    @JvmStatic
    external fun nativeInvokeCallbackWithStringArray(key: Long, paths: Array<String>)

    // Issue #4773 — drag & drop. The drop callback delivers the parsed payload
    // (any of text/files/urls may be null); the drag provider returns the
    // payload string for one representation when a drag begins.
    @JvmStatic
    external fun nativeInvokeDropCallback(
        key: Long,
        text: String?,
        files: Array<String>?,
        urls: Array<String>?,
    )

    @JvmStatic
    external fun nativeInvokeDragProvider(key: Long): String

    // Issue #480 — TreeView row-tap callback.
    @JvmStatic
    external fun nativeTreeRowTapped(widgetHandle: Long, id: String)

    // Issue #658 v2-A — WebView callbacks routed through PerryWebViewClient.
    /// Sync intercept; returns `true` to allow nav, `false` to cancel.
    @JvmStatic
    external fun nativeWebViewShouldNavigate(widgetHandle: Long, url: String): Boolean

    /// Page finished loading — `widgetHandle` selects the WEBVIEW_STATES
    /// entry holding the user's `onLoaded` closure pointer.
    @JvmStatic
    external fun nativeWebViewLoaded(widgetHandle: Long, url: String)

    /// Main-frame load error — routes to the user's `onError` closure.
    @JvmStatic
    external fun nativeWebViewError(widgetHandle: Long, code: Long, message: String)

    /// `evaluateJavascript` completion — `callbackKey` is the per-call key
    /// the Rust side stashed when the call was issued.
    @JvmStatic
    external fun nativeWebViewEvalResult(callbackKey: Long, result: String)

    @JvmStatic
    external fun nativeFileDialogResult(key: Long, content: String?)

    @JvmStatic
    external fun nativeGetMenuItemCount(menuHandle: Long): Int

    @JvmStatic
    external fun nativeGetMenuItemTitle(menuHandle: Long, index: Int): String

    @JvmStatic
    external fun nativeMenuItemSelected(menuHandle: Long, index: Int)

    // =====================================================================
    // Notifications (issues #95/#96/#97) — local + scheduled notifications.
    //
    // These static methods are invoked over JNI from
    // perry-ui-android/src/system.rs; the Kotlin signatures must keep
    // matching the JNI descriptors used there exactly:
    //   sendNotification                (Landroid/app/Activity;Ljava/lang/String;Ljava/lang/String;)V
    //   cancelNotification              (Landroid/app/Activity;Ljava/lang/String;)V
    //   registerForRemoteNotifications  (Landroid/app/Activity;)V
    //   scheduleInterval                (Landroid/app/Activity;Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;DZ)V
    //   scheduleCalendar                (Landroid/app/Activity;Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;D)V
    // =====================================================================

    private const val NOTIFICATION_TAG = "PerryNotification"

    /** Channel every Perry notification posts to (required since API 26). */
    private const val NOTIFICATION_CHANNEL_ID = "perry_default"

    /**
     * Identifier used by `notificationSend` (the TS surface passes no id for
     * plain sends). Matches the fixed "perry_notification" request identifier
     * the Apple implementations use, so
     * `notificationCancel("perry_notification")` behaves the same on both
     * platforms.
     */
    private const val DEFAULT_NOTIFICATION_ID = "perry_notification"

    private fun ensureNotificationChannel(context: Context) {
        if (android.os.Build.VERSION.SDK_INT < 26) return
        val mgr = context.getSystemService(Context.NOTIFICATION_SERVICE)
            as android.app.NotificationManager
        if (mgr.getNotificationChannel(NOTIFICATION_CHANNEL_ID) == null) {
            mgr.createNotificationChannel(
                android.app.NotificationChannel(
                    NOTIFICATION_CHANNEL_ID,
                    "Notifications",
                    android.app.NotificationManager.IMPORTANCE_DEFAULT
                )
            )
        }
    }

    /**
     * The template ships no launcher icon of its own (the manifest has no
     * `android:icon`), so fall back to a system drawable when
     * `applicationInfo.icon` is unset — the system rejects notifications
     * without a valid small icon.
     */
    private fun notificationSmallIcon(context: Context): Int {
        val appIcon = context.applicationInfo.icon
        return if (appIcon != 0) appIcon else android.R.drawable.ic_dialog_info
    }

    /** True when we may post notifications (POST_NOTIFICATIONS on API 33+). */
    private fun canPostNotifications(context: Context): Boolean {
        if (android.os.Build.VERSION.SDK_INT < 33) return true
        return ContextCompat.checkSelfPermission(
            context, Manifest.permission.POST_NOTIFICATIONS
        ) == PackageManager.PERMISSION_GRANTED
    }

    /**
     * Build + post one notification. Shared by `sendNotification` (immediate)
     * and `PerryNotificationReceiver` (AlarmManager-scheduled fire, so this
     * must work from a receiver context with `PerryBridge.init` never run).
     * `id` becomes the notification *tag*, which is what lets
     * `cancelNotification` key the shade entry by the same string id.
     */
    fun postNotification(context: Context, id: String, title: String, body: String) {
        if (!canPostNotifications(context)) {
            Log.w(
                NOTIFICATION_TAG,
                "notification '$id' dropped: POST_NOTIFICATIONS not granted (Android 13+). " +
                    "PerryActivity requests it at launch; the user denied it."
            )
            return
        }
        ensureNotificationChannel(context)

        // Tapping the banner must launch the Activity directly — apps
        // targeting API 31+ may not start an activity from a
        // receiver/service reached via a notification tap ("trampoline"
        // block), so the tap PendingIntent points straight at
        // PerryActivity, which forwards the id extra to
        // nativeNotificationTap (issue #97). No data URI here: PerryActivity
        // forwards `intent.data` to the deep-link callbacks (#583), and a
        // synthetic URI would fire those spuriously. Distinct ids get
        // distinct PendingIntents via the request code instead (extras
        // don't participate in PendingIntent matching).
        val tapIntent = Intent(context, PerryActivity::class.java).apply {
            addFlags(Intent.FLAG_ACTIVITY_SINGLE_TOP)
            putExtra(PerryNotificationReceiver.EXTRA_ID, id)
        }
        val tapPending = android.app.PendingIntent.getActivity(
            context,
            id.hashCode(),
            tapIntent,
            android.app.PendingIntent.FLAG_UPDATE_CURRENT or
                android.app.PendingIntent.FLAG_IMMUTABLE
        )

        val notification =
            androidx.core.app.NotificationCompat.Builder(context, NOTIFICATION_CHANNEL_ID)
                .setSmallIcon(notificationSmallIcon(context))
                .setContentTitle(title)
                .setContentText(body)
                .setStyle(androidx.core.app.NotificationCompat.BigTextStyle().bigText(body))
                .setAutoCancel(true)
                .setContentIntent(tapPending)
                .build()
        try {
            androidx.core.app.NotificationManagerCompat.from(context).notify(id, 0, notification)
        } catch (e: SecurityException) {
            // Permission revoked between the check above and the call.
            Log.w(NOTIFICATION_TAG, "notify('$id') failed: ${e.message}")
        }
    }

    /** JNI: `notificationSend(title, body)` — post immediately. */
    @JvmStatic
    fun sendNotification(activity: Activity, title: String, body: String) {
        postNotification(activity, DEFAULT_NOTIFICATION_ID, title, body)
    }

    /**
     * Build the AlarmManager PendingIntent for a scheduled notification id.
     * The `perry-notification://fire/<id>` data URI is what makes each id
     * its own PendingIntent (extras are ignored by `Intent.filterEquals`),
     * so schedule/cancel pairs resolve to the same alarm.
     */
    private fun scheduledFirePendingIntent(
        context: Context,
        id: String,
        title: String?,
        body: String?,
        flags: Int,
    ): android.app.PendingIntent? {
        val intent = Intent(context, PerryNotificationReceiver::class.java).apply {
            action = PerryNotificationReceiver.ACTION_FIRE
            data = Uri.parse("perry-notification://fire/" + Uri.encode(id))
            putExtra(PerryNotificationReceiver.EXTRA_ID, id)
            if (title != null) putExtra(PerryNotificationReceiver.EXTRA_TITLE, title)
            if (body != null) putExtra(PerryNotificationReceiver.EXTRA_BODY, body)
        }
        return android.app.PendingIntent.getBroadcast(
            context, 0, intent, flags or android.app.PendingIntent.FLAG_IMMUTABLE
        )
    }

    /**
     * JNI: schedule a fire-after-N-seconds notification (issue #96).
     * Inexact timing: exact alarms need the SCHEDULE_EXACT_ALARM special
     * permission on API 31+, which a notification banner doesn't justify.
     * Repeating intervals under a minute are clamped to 60s by the OS.
     */
    @JvmStatic
    fun scheduleInterval(
        activity: Activity,
        id: String,
        title: String,
        body: String,
        seconds: Double,
        repeats: Boolean,
    ) {
        val alarm = activity.getSystemService(Context.ALARM_SERVICE) as android.app.AlarmManager
        val delayMs = (seconds * 1000.0).toLong().coerceAtLeast(0L)
        val triggerAt = System.currentTimeMillis() + delayMs
        val pending = scheduledFirePendingIntent(
            activity, id, title, body, android.app.PendingIntent.FLAG_UPDATE_CURRENT
        ) ?: return
        if (repeats) {
            alarm.setRepeating(android.app.AlarmManager.RTC_WAKEUP, triggerAt, delayMs, pending)
        } else {
            alarm.setAndAllowWhileIdle(android.app.AlarmManager.RTC_WAKEUP, triggerAt, pending)
        }
    }

    /** JNI: schedule a fire-at-wallclock-ms notification (issue #96). */
    @JvmStatic
    fun scheduleCalendar(
        activity: Activity,
        id: String,
        title: String,
        body: String,
        timestampMs: Double,
    ) {
        val alarm = activity.getSystemService(Context.ALARM_SERVICE) as android.app.AlarmManager
        val pending = scheduledFirePendingIntent(
            activity, id, title, body, android.app.PendingIntent.FLAG_UPDATE_CURRENT
        ) ?: return
        alarm.setAndAllowWhileIdle(
            android.app.AlarmManager.RTC_WAKEUP, timestampMs.toLong(), pending
        )
    }

    /** JNI: cancel a scheduled and/or already-delivered notification by id. */
    @JvmStatic
    fun cancelNotification(activity: Activity, id: String) {
        // Cancel the pending AlarmManager schedule, if one exists.
        val alarm = activity.getSystemService(Context.ALARM_SERVICE) as android.app.AlarmManager
        val pending = scheduledFirePendingIntent(
            activity, id, null, null, android.app.PendingIntent.FLAG_NO_CREATE
        )
        if (pending != null) {
            alarm.cancel(pending)
            pending.cancel()
        }
        // Remove the delivered notification (tag = id) from the shade.
        androidx.core.app.NotificationManagerCompat.from(activity).cancel(id, 0)
    }

    /**
     * JNI: `notificationRegisterRemote` (issue #95). Real FCM registration
     * needs app-level Firebase wiring the scaffold intentionally does not
     * ship: a `google-services.json`, the `com.google.gms.google-services`
     * Gradle plugin, the `com.google.firebase:firebase-messaging`
     * dependency, and a `FirebaseMessagingService` subclass. Until the app
     * adds those, this logs exactly what's missing instead of failing the
     * JNI lookup silently.
     *
     * When you do add Firebase to the generated project, forward the token
     * and payloads to the native side from your service:
     *   onNewToken(token)      → PerryBridge.nativeNotificationToken(token)
     *   onMessageReceived(msg) → PerryBridge.nativeNotificationReceive(json)
     *                            and/or nativeNotificationBackgroundReceive(json)
     */
    @JvmStatic
    fun registerForRemoteNotifications(activity: Activity) {
        Log.w(
            NOTIFICATION_TAG,
            "registerForRemoteNotifications: this app has no Firebase Messaging. Add " +
                "google-services.json, the google-services Gradle plugin and " +
                "com.google.firebase:firebase-messaging, then forward onNewToken / " +
                "onMessageReceived to PerryBridge.nativeNotificationToken / " +
                "nativeNotificationReceive. See https://github.com/PerryTS/perry/issues/95"
        )
    }

    /**
     * Forward a notification-tap Intent (built by `postNotification`) to the
     * native tap callback. Called from PerryActivity.onCreate/onNewIntent.
     * Returns true when a tap id was present and dispatched.
     */
    @JvmStatic
    fun handleNotificationTapIntent(intent: Intent?): Boolean {
        val id = intent?.getStringExtra(PerryNotificationReceiver.EXTRA_ID) ?: return false
        return try {
            nativeNotificationTap(id)
            true
        } catch (_: UnsatisfiedLinkError) {
            // Cold start: the tap arrived before the native library loaded,
            // so no JS callback can be registered yet. Same v1 limitation as
            // FCM cold delivery (#98).
            Log.w(
                NOTIFICATION_TAG,
                "notification tap for '$id' before native lib load; onTap callback skipped"
            )
            false
        }
    }

    // Notification native callbacks (issues #95/#97/#98) — exported by
    // perry-ui-android/src/system.rs as
    // Java_com_perry_app_PerryBridge_nativeNotification{Tap,Token,Receive,BackgroundReceive}.

    /** User tapped a delivered notification; `id` is the notification id. */
    @JvmStatic
    external fun nativeNotificationTap(id: String)

    /** FCM registration token (initial fetch + rotations). */
    @JvmStatic
    external fun nativeNotificationToken(token: String)

    /** Foreground push payload as a JSON string. */
    @JvmStatic
    external fun nativeNotificationReceive(payloadJson: String)

    /** Background push payload as a JSON string. */
    @JvmStatic
    external fun nativeNotificationBackgroundReceive(payloadJson: String)
}
