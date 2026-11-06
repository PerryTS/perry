package com.perry.app;

import android.app.Activity;
import android.app.Instrumentation;
import android.content.Intent;
import android.os.Bundle;
import android.os.SystemClock;
import android.view.View;
import android.view.ViewGroup;
import android.widget.TextView;

public final class TabBarTest extends Instrumentation {
    private static native void select(long index);
    private static native void append();
    private static View root;
    private static int callbacks;
    private static int lastIndex = -1;
    private static boolean contentVisibleDuringCallback;
    public static void recordSelection(int index) {
        callbacks++;
        lastIndex = index;
        String label = index == 0 ? "retained state" : index == 1 ? "SECOND CONTENT" : "THIRD CONTENT";
        TextView content = find(root, label);
        contentVisibleDuringCallback = content != null && content.isShown();
        // Re-enter the real native API from a UI callback: no thread-local state/deadlock.
        select(index);
    }
    @Override public void onCreate(Bundle args) { super.onCreate(args); start(); }
    private static void check(boolean ok, String message) {
        if (!ok) throw new AssertionError(message);
    }
    private static TextView find(View view, String label) {
        if (view instanceof TextView && ((TextView)view).getText().toString().equals(label))
            return (TextView)view;
        if (view instanceof ViewGroup) {
            ViewGroup group = (ViewGroup)view;
            for (int i=0; i<group.getChildCount(); i++) {
                TextView result = find(group.getChildAt(i), label);
                if (result != null) return result;
            }
        }
        return null;
    }
    private void ui(Runnable action) {
        Throwable[] failure = new Throwable[1];
        runOnMainSync(() -> { try { action.run(); } catch(Throwable e) { failure[0] = e; } });
        if (failure[0] != null) throw new AssertionError(failure[0]);
    }
    private void settle() { SystemClock.sleep(200); waitForIdleSync(); }
    @Override public void onStart() {
        Bundle result = new Bundle();
        try {
            Intent intent = new Intent(getTargetContext(), PerryActivity.class);
            intent.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK);
            Activity activity = startActivitySync(intent);
            root = activity.getWindow().getDecorView();
            for(int i=0; i<100 && find(root,"A")==null; i++) settle();
            settle();
            TextView first = find(root,"FIRST CONTENT");
            TextView second = find(root,"SECOND CONTENT");
            ui(() -> {
                check(first != null, "first content was discarded");
                check(second != null, "second content was discarded");
                check(first.isShown() && first.getHeight()>0 && first.getWidth()>0, "initial content invisible");
                check(!second.isShown(), "inactive content visible");
                check(callbacks == 0, "initial selection invoked callback");
                first.setText("retained state");
                find(root,"B").performClick();
                check(second.isShown() && !first.isShown(), "tap did not switch content");
                check(callbacks == 1 && lastIndex == 1 && contentVisibleDuringCallback, "tap callback/order wrong");
            });
            // Native thread path after attachment, without generating another callback.
            select(0); settle();
            ui(() -> {
                check(first.isShown() && !second.isShown(), "programmatic selection failed");
                check(find(root,"retained state") == first, "content state was rebuilt");
                check(callbacks == 1, "programmatic selection invoked callback");
            });
            for(long invalid : new long[] {-1, 2, Long.MAX_VALUE, Long.MIN_VALUE}) select(invalid);
            ui(() -> check(first.isShown(), "invalid index changed selection"));
            append(); settle();
            ui(() -> {
                check(first.isShown(), "adding a tab changed selection");
                check(!find(root,"THIRD CONTENT").isShown(), "new inactive tab visible");
                find(root,"C").performClick();
                check(find(root,"THIRD CONTENT").isShown(), "dynamic tab failed");
                check(callbacks == 2 && lastIndex == 2 && contentVisibleDuringCallback, "dynamic callback wrong");
                find(root,"A").performClick();
                check(first.isShown() && callbacks == 3 && lastIndex == 0, "return to retained tab failed");
            });
            result.putString("stream", "\nPASS: production Rust TabBar ABI, initial content, taps/callbacks, retained state, programmatic/invalid selection, dynamic tab\n");
            finish(0,result);
        } catch(Throwable error) {
            result.putString("stream", "\nFAIL: " + android.util.Log.getStackTraceString(error));
            finish(1,result);
        }
    }
}
