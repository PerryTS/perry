package com.perry.app

import android.content.Context
import android.graphics.Color
import android.os.Looper
import android.view.Gravity
import android.view.MotionEvent
import android.view.View
import android.view.ViewGroup
import android.widget.FrameLayout
import android.widget.LinearLayout

/** Horizontal panes with draggable dividers. Weights retain proportions on resize. */
class PerrySplitView(context: Context) : LinearLayout(context) {
    private val panes = mutableListOf<FrameLayout>()
    private val density = resources.displayMetrics.density

    init {
        orientation = HORIZONTAL
        layoutParams = ViewGroup.LayoutParams(LayoutParams.MATCH_PARENT, LayoutParams.MATCH_PARENT)
    }

    fun addPane(child: View) {
        // Native code can add panes after the container has been attached.
        if (Looper.myLooper() != Looper.getMainLooper()) {
            post { addPane(child) }
            return
        }
        if (child === this || panes.any { it === child || it.getChildAt(0) === child }) return
        (child.parent as? ViewGroup)?.removeView(child)
        val pane = FrameLayout(context)
        pane.addView(child, FrameLayout.LayoutParams(LayoutParams.MATCH_PARENT, LayoutParams.MATCH_PARENT))
        if (panes.isNotEmpty()) addView(divider(panes.last(), pane))
        panes.add(pane)
        addView(pane, LayoutParams(0, LayoutParams.MATCH_PARENT, 1f))
    }

    private fun divider(left: FrameLayout, right: FrameLayout): View {
        // A narrow visible rule inside a larger touch target.
        val handle = FrameLayout(context)
        handle.contentDescription = "Resize split panes"
        handle.layoutParams = LayoutParams((24 * density).toInt(), LayoutParams.MATCH_PARENT)
        val rule = View(context)
        rule.setBackgroundColor(Color.GRAY)
        handle.addView(rule, FrameLayout.LayoutParams(
            (density * 2).toInt().coerceAtLeast(1), LayoutParams.MATCH_PARENT, Gravity.CENTER))
        var startX = 0f
        var startWidth = 0f
        var pairWidth = 0f
        var pairWeight = 0f
        handle.setOnTouchListener { _, event ->
            when (event.actionMasked) {
                MotionEvent.ACTION_DOWN -> {
                    startX = event.rawX
                    startWidth = left.width.toFloat()
                    pairWidth = (left.width + right.width).toFloat()
                    pairWeight = (left.layoutParams as LayoutParams).weight +
                        (right.layoutParams as LayoutParams).weight
                    parent?.requestDisallowInterceptTouchEvent(true)
                    true
                }
                MotionEvent.ACTION_MOVE -> {
                    if (pairWidth > 0f) {
                        val minimum = (48 * density).coerceAtMost(pairWidth / 2)
                        val width = (startWidth + event.rawX - startX)
                            .coerceIn(minimum, pairWidth - minimum)
                        val leftParams = left.layoutParams as LayoutParams
                        val rightParams = right.layoutParams as LayoutParams
                        leftParams.weight = pairWeight * width / pairWidth
                        rightParams.weight = pairWeight - leftParams.weight
                        left.layoutParams = leftParams
                        right.layoutParams = rightParams
                    }
                    true
                }
                MotionEvent.ACTION_UP, MotionEvent.ACTION_CANCEL -> {
                    parent?.requestDisallowInterceptTouchEvent(false)
                    if (event.actionMasked == MotionEvent.ACTION_UP) handle.performClick()
                    true
                }
                else -> false
            }
        }
        return handle
    }
}
