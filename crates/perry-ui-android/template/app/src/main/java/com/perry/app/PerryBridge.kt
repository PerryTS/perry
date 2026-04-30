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

    @JvmStatic
    external fun nativeInvokeCallback0(key: Long)

    @JvmStatic
    external fun nativeInvokeCallback1(key: Long, arg: Double)

    @JvmStatic
    external fun nativeInvokeCallback2(key: Long, arg1: Double, arg2: Double)

    @JvmStatic
    external fun nativeInvokeCallbackWithString(key: Long, text: String)

    @JvmStatic
    external fun nativeFileDialogResult(key: Long, content: String?)

    @JvmStatic
    external fun nativeGetMenuItemCount(menuHandle: Long): Int

    @JvmStatic
    external fun nativeGetMenuItemTitle(menuHandle: Long, index: Int): String

    @JvmStatic
    external fun nativeMenuItemSelected(menuHandle: Long, index: Int)
}
