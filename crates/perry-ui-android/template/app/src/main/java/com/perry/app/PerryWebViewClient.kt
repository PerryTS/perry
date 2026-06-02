package com.perry.app

import android.webkit.WebResourceError
import android.webkit.WebResourceRequest
import android.webkit.WebView
import android.webkit.WebViewClient
import android.webkit.ValueCallback

/**
 * Custom WebViewClient that proxies the navigation hooks Perry's
 * [com.perry.app.PerryBridge] needs back into Rust via JNI. Each
 * Perry WebView gets its own instance of this class wired with a
 * `widgetHandle` that lets the native side route notifications back
 * to the right widget in `WEBVIEW_STATES`.
 *
 * Issue #658 v2-A.
 *
 * Mirror of macOS / iOS / Windows WKNavigationDelegate-style
 * callbacks:
 *  - `shouldOverrideUrlLoading` ↔ `decidePolicyForNavigationAction`
 *    on Apple platforms; sync `Boolean` return cancels navigation
 *    when `nativeWebViewShouldNavigate` returns `false`.
 *  - `onPageFinished` ↔ `didFinishNavigation` — fires
 *    `nativeWebViewLoaded(handle, url)` for the user's `onLoaded`
 *    closure.
 *  - `onReceivedError` ↔ `didFailNavigation:withError:` — fires
 *    `nativeWebViewError(handle, code, msg)` for the user's
 *    `onError` closure.
 *
 * The matching `ValueCallback<String>` for `evaluateJavascript` is
 * declared inside `PerryBridge` (not here) so the Rust side can
 * register a per-call callback key for any JS eval without
 * subclassing this class per-call.
 */
class PerryWebViewClient(private val widgetHandle: Long) : WebViewClient() {

    override fun shouldOverrideUrlLoading(view: WebView, request: WebResourceRequest): Boolean {
        val url = request.url?.toString() ?: return false
        // Returning `true` cancels the navigation — we cancel iff the native
        // side returns `false` from its `onShouldNavigate` callback.
        return !PerryBridge.nativeWebViewShouldNavigate(widgetHandle, url)
    }

    override fun onPageFinished(view: WebView, url: String?) {
        super.onPageFinished(view, url)
        PerryBridge.nativeWebViewLoaded(widgetHandle, url ?: "")
    }

    override fun onReceivedError(
        view: WebView,
        request: WebResourceRequest,
        error: WebResourceError
    ) {
        super.onReceivedError(view, request, error)
        // Per Android's `WebViewClient` contract, `onReceivedError` only
        // fires for the main frame (sub-resource errors don't bubble).
        // Both error code and description map cleanly to Perry's
        // `(code: number, message: string)` contract.
        if (request.isForMainFrame) {
            PerryBridge.nativeWebViewError(
                widgetHandle,
                error.errorCode.toLong(),
                error.description?.toString() ?: ""
            )
        }
    }
}

/**
 * `ValueCallback<String>` for `WebView.evaluateJavascript(js, callback)`.
 * Each `evaluateJs` call from Rust constructs one of these wired to a
 * per-call `callbackKey` that routes the JS string result back through
 * the native `nativeWebViewEvalResult` JNI method, where the Rust side
 * looks up the user's TS closure and invokes it.
 */
class PerryWebViewEvalCallback(private val callbackKey: Long) : ValueCallback<String> {
    override fun onReceiveValue(value: String?) {
        PerryBridge.nativeWebViewEvalResult(callbackKey, value ?: "")
    }
}
