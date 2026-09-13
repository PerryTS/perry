package com.perry.app

import android.content.Context
import android.view.Gravity
import android.view.View
import android.widget.FrameLayout

/**
 * A one-child layout that reproduces CSS `max-width` + `margin: auto`.
 *
 * A plain Android View/LinearLayout child has no maximum-width property, so
 * `widgetSetMaxWidth` wraps the child in this layout. It fills the parent's
 * width, but measures its child at no more than `maxWidthPx` and centers it —
 * below the cap the child fills, at and above it the child holds at the cap with
 * even side gutters.
 *
 * `maxWidthPx` is set from Rust via JNI after construction.
 */
class PerryMaxWidthLayout(context: Context) : FrameLayout(context) {
    var maxWidthPx: Int = 0

    init {
        // Center the single child so the gutters split evenly once capped.
        // (Gravity is applied to the child's FrameLayout.LayoutParams on add.)
    }

    override fun addView(child: View, index: Int, params: LayoutParams?) {
        val lp = params ?: LayoutParams(LayoutParams.MATCH_PARENT, LayoutParams.WRAP_CONTENT)
        lp.gravity = Gravity.CENTER_HORIZONTAL
        super.addView(child, index, lp)
    }

    override fun onMeasure(widthMeasureSpec: Int, heightMeasureSpec: Int) {
        val available = MeasureSpec.getSize(widthMeasureSpec)
        val cap = if (maxWidthPx in 1 until available) maxWidthPx else available
        // Measure the child at no more than the cap; the layout itself still
        // spans the parent so the centering gutters live inside it.
        val childSpec = MeasureSpec.makeMeasureSpec(cap, MeasureSpec.EXACTLY)
        super.onMeasure(childSpec, heightMeasureSpec)
        // Report the full available width so the bin fills the parent row.
        setMeasuredDimension(available, measuredHeight)
    }
}
