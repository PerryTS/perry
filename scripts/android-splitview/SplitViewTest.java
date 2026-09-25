package com.perry.app;

import android.app.Activity;
import android.app.Instrumentation;
import android.content.Intent;
import android.os.Bundle;
import android.os.SystemClock;
import android.view.MotionEvent;
import android.view.View;
import android.view.ViewGroup;
import android.widget.TextView;

public final class SplitViewTest extends Instrumentation {
    private static native void addThird();
    private PerrySplitView split;

    @Override public void onCreate(Bundle arguments) { super.onCreate(arguments); start(); }
    private static void check(boolean condition, String message) {
        if (!condition) throw new AssertionError(message);
    }
    private PerrySplitView find(View view) {
        if (view instanceof PerrySplitView) return (PerrySplitView) view;
        if (view instanceof ViewGroup) {
            ViewGroup group = (ViewGroup) view;
            for (int i = 0; i < group.getChildCount(); i++) {
                PerrySplitView result = find(group.getChildAt(i));
                if (result != null) return result;
            }
        }
        return null;
    }
    private void settle() { SystemClock.sleep(300); waitForIdleSync(); }
    private void drag(float delta) {
        runOnMainSync(() -> {
            View divider = split.getChildAt(1);
            long time = SystemClock.uptimeMillis();
            for (int action : new int[] {MotionEvent.ACTION_DOWN, MotionEvent.ACTION_MOVE, MotionEvent.ACTION_UP}) {
                MotionEvent event = MotionEvent.obtain(time, time + action * 10, action,
                    12 + (action == MotionEvent.ACTION_DOWN ? 0 : delta), 30, 0);
                divider.dispatchTouchEvent(event);
                event.recycle();
            }
        });
        settle();
    }
    @Override public void onStart() {
        Bundle result = new Bundle();
        try {
            Intent launch = new Intent(getTargetContext(), PerryActivity.class);
            launch.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK);
            Activity activity = startActivitySync(launch);
            for (int i = 0; i < 100 && split == null; i++) {
                runOnMainSync(() -> split = find(activity.getWindow().getDecorView()));
                SystemClock.sleep(50);
            }
            check(split != null, "Rust SplitView constructor did not attach a view");
            settle();
            runOnMainSync(() -> {
                check(split.getChildCount() == 3, "expected two panes and a divider");
                check(split.getWidth() > 0 && split.getHeight() > 0, "split has no area");
                ViewGroup left = (ViewGroup) split.getChildAt(0);
                ViewGroup right = (ViewGroup) split.getChildAt(2);
                check(((TextView) left.getChildAt(0)).getText().toString().equals("LEFT"), "left missing");
                check(((TextView) right.getChildAt(0)).getText().toString().equals("RIGHT"), "right missing");
                check(left.getWidth() > 0 && right.getWidth() > 0, "pane has no width");
                check(left.getHeight() == split.getHeight(), "pane does not fill height");
                check(left.getRight() <= right.getLeft(), "panes overlap");
            });
            int before = split.getChildAt(0).getWidth();
            drag(50);
            check(split.getChildAt(0).getWidth() > before, "drag did not enlarge left pane");
            drag(-10000);
            check(split.getChildAt(0).getWidth() > 0, "drag collapsed left pane");
            drag(10000);
            check(split.getChildAt(2).getWidth() > 0, "drag collapsed right pane");
            // Call the production ABI from a non-UI thread after attachment.
            addThird();
            settle();
            runOnMainSync(() -> {
                check(split.getChildCount() == 5, "dynamic pane not added");
                ViewGroup third = (ViewGroup) split.getChildAt(4);
                check(((TextView) third.getChildAt(0)).getText().toString().equals("THIRD"), "third missing");
                check(third.getWidth() > 0, "third pane has no width");
                check(third.getRight() == split.getWidth(), "panes do not fill width");
            });
            int fullWidth = split.getWidth();
            runOnMainSync(() -> {
                ViewGroup.LayoutParams params = split.getLayoutParams();
                params.width = fullWidth / 2;
                split.setLayoutParams(params);
            });
            settle();
            runOnMainSync(() -> {
                check(split.getWidth() == fullWidth / 2, "container did not resize");
                for (int i = 0; i < split.getChildCount(); i += 2)
                    check(split.getChildAt(i).getWidth() > 0, "resize collapsed a pane");
                check(split.getChildAt(4).getRight() == split.getWidth(), "resized panes overflow");
            });
            result.putString("stream", "\nPASS: production Rust SplitView ABI, two visible panes, drag/clamps, dynamic third pane, resize\n");
            finish(0, result);
        } catch (Throwable error) {
            result.putString("stream", "\nFAIL: " + android.util.Log.getStackTraceString(error));
            finish(1, result);
        }
    }
}
