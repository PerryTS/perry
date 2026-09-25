package com.perry.app;

import android.app.Activity;
import android.app.Instrumentation;
import android.content.Intent;
import android.content.res.Configuration;
import android.os.Bundle;
import android.os.ParcelFileDescriptor;
import android.os.SystemClock;
import android.widget.Button;
import android.widget.LinearLayout;
import android.widget.TextView;
import java.io.FileInputStream;
import java.util.concurrent.atomic.AtomicInteger;

/** Real Activity lifecycle regression; only the compiled app entry point is a JNI fixture. */
public final class NightModeTest extends Instrumentation {
    private static final AtomicInteger starts = new AtomicInteger();
    private static volatile Activity owner;
    private static volatile TextView label;
    private static volatile Button button;

    // Called by libperry_app.so from the production perry-native thread.
    public static void onNativeMain() {
        starts.incrementAndGet();
        Activity activity = PerryBridge.getActivity();
        activity.runOnUiThread(() -> {
            AtomicInteger taps = new AtomicInteger();
            LinearLayout content = new LinearLayout(activity);
            content.setOrientation(LinearLayout.VERTICAL);
            TextView text = new TextView(activity);
            text.setText("taps=0");
            Button tap = new Button(activity);
            tap.setText("tap");
            tap.setOnClickListener(view -> text.setText("taps=" + taps.incrementAndGet()));
            content.addView(text);
            content.addView(tap);
            activity.setContentView(content);
            label = text;
            button = tap;
            owner = activity;
        });
    }

    @Override public void onCreate(Bundle arguments) {
        super.onCreate(arguments);
        start();
    }

    private void shell(String command) throws Exception {
        try (ParcelFileDescriptor fd = getUiAutomation().executeShellCommand(command);
             FileInputStream input = new FileInputStream(fd.getFileDescriptor())) {
            byte[] buffer = new byte[1024];
            while (input.read(buffer) != -1) {}
        }
    }

    private static void check(boolean condition, String message) {
        if (!condition) throw new AssertionError(message);
    }

    private void awaitNight(int expected) {
        long deadline = SystemClock.uptimeMillis() + 10000;
        while (SystemClock.uptimeMillis() < deadline) {
            if (owner != null && (owner.getResources().getConfiguration().uiMode
                    & Configuration.UI_MODE_NIGHT_MASK) == expected) {
                waitForIdleSync();
                return;
            }
            SystemClock.sleep(50);
        }
        throw new AssertionError("Activity never received night mode " + expected);
    }

    @Override public void onStart() {
        Bundle result = new Bundle();
        try {
            shell("cmd uimode night no");
            Intent launch = new Intent(getTargetContext(), PerryActivity.class);
            launch.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK);
            Activity initial = startActivitySync(launch);
            long deadline = SystemClock.uptimeMillis() + 10000;
            while (owner == null && SystemClock.uptimeMillis() < deadline) SystemClock.sleep(50);
            check(owner == initial, "nativeMain did not install the initial UI");
            awaitNight(Configuration.UI_MODE_NIGHT_NO);
            runOnMainSync(() -> { button.performClick(); button.performClick(); });
            TextView initialLabel = label;
            Button initialButton = button;
            int pid = android.os.Process.myPid();
            for (int mode : new int[] {Configuration.UI_MODE_NIGHT_YES,
                    Configuration.UI_MODE_NIGHT_NO, Configuration.UI_MODE_NIGHT_YES}) {
                shell("cmd uimode night " + (mode == Configuration.UI_MODE_NIGHT_YES ? "yes" : "no"));
                awaitNight(mode);
                // Allow a scheduled recreation/native startup to complete before checking.
                SystemClock.sleep(500);
                waitForIdleSync();
                check(starts.get() == 1, "nativeMain ran " + starts.get() + " times in pid " + pid);
                check(owner == initial && !initial.isDestroyed(), "Activity was recreated");
                check(android.os.Process.myPid() == pid, "process changed");
                runOnMainSync(() -> {
                    check(label == initialLabel && button == initialButton, "views replaced");
                    check(label.isAttachedToWindow(), "original UI detached");
                    check(label.getText().toString().equals("taps=2"), "counter reset");
                });
            }
            runOnMainSync(() -> {
                button.performClick();
                check(label.getText().toString().equals("taps=3"), "callback stopped working");
            });
            result.putString("stream", "\nPASS: three night-mode changes, one nativeMain, same Activity/views, taps=3\n");
            finish(0, result);
        } catch (Throwable error) {
            result.putString("stream", "\nFAIL: " + android.util.Log.getStackTraceString(error));
            finish(1, result);
        }
    }
}
