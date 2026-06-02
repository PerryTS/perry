package com.perry.app

import android.content.Context
import android.os.Handler
import android.os.Looper
import android.util.Log
import androidx.work.Worker
import androidx.work.WorkerParameters
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit

/**
 * WorkManager-backed runner for `perry/background` (issue #538).
 *
 * `doWork` is invoked on a worker thread; it looks up the user's registered
 * handler closure on the UI thread (the only thread Perry's runtime is
 * pinned to), invokes it via `nativeInvokeCallback0`, and waits up to 10 s
 * for any synchronous follow-up work before returning Success.
 */
class PerryBackgroundWorker(
    appContext: Context,
    workerParams: WorkerParameters
) : Worker(appContext, workerParams) {

    override fun doWork(): Result {
        val identifier = inputData.getString("identifier") ?: return Result.success()
        val callbackKey = PerryBridge.backgroundLookupCallbackKey(identifier)
        if (callbackKey == 0L) {
            Log.w("PerryBackground", "no handler registered for $identifier — skipping")
            return Result.success()
        }

        // Perry's runtime expects callbacks on the UI thread (its arena is
        // thread-pinned). Bounce there, fire the closure, wait briefly for
        // synchronous Promise chains to drain, then return.
        val latch = CountDownLatch(1)
        Handler(Looper.getMainLooper()).post {
            try {
                PerryBridge.nativeInvokeCallback0(callbackKey)
            } catch (e: Throwable) {
                Log.e("PerryBackground", "handler threw: ${e.message}")
            } finally {
                latch.countDown()
            }
        }
        latch.await(10, TimeUnit.SECONDS)
        return Result.success()
    }
}
