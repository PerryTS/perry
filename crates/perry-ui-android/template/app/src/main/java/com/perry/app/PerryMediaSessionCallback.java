package com.perry.app;

import android.support.v4.media.session.MediaSessionCompat;

/**
 * Bridges MediaSessionCompat transport-control events (lock-screen,
 * Bluetooth headset, Wear OS, Android Auto) into the Perry runtime.
 *
 * Constructed from native code via JNI in
 * {@code crates/perry-ui-android/src/media_playback.rs::ensure_session}
 * and registered with {@code MediaSessionCompat.setCallback(...)}.
 *
 * Each handler routes to the first live MediaPlayer handle. The native
 * thunks ({@code nativeMediaSessionPlay} et al.) call back into the
 * existing {@code play / pause / stop / seek} entry points so the
 * registered TS {@code onStateChange} closure fires the same way it
 * would if the user pressed an in-app button.
 */
public class PerryMediaSessionCallback extends MediaSessionCompat.Callback {

    public static native void nativeMediaSessionPlay();
    public static native void nativeMediaSessionPause();
    public static native void nativeMediaSessionStop();
    public static native void nativeMediaSessionSeekTo(long positionMs);

    @Override
    public void onPlay() {
        nativeMediaSessionPlay();
    }

    @Override
    public void onPause() {
        nativeMediaSessionPause();
    }

    @Override
    public void onStop() {
        nativeMediaSessionStop();
    }

    @Override
    public void onSeekTo(long pos) {
        nativeMediaSessionSeekTo(pos);
    }

    @Override
    public void onSkipToNext() {
        // No-op for now — the perry/media surface doesn't yet expose a
        // queue / next-track callback. Bluetooth devices that send this
        // event silently ignore the lack of advancement.
    }

    @Override
    public void onSkipToPrevious() {
        // See onSkipToNext().
    }
}
