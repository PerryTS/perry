package com.perry.app

import android.content.Context
import android.view.Gravity
import android.view.View
import android.view.ViewGroup
import android.widget.FrameLayout
import android.widget.LinearLayout
import android.widget.TextView

/** Retains each tab's content; selection changes visibility without rebuilding widgets. */
class PerryTabBar(context: Context, private val callbackKey: Long) : LinearLayout(context) {
    private val contentHost = FrameLayout(context)
    private val titles = LinearLayout(context)
    private val contents = mutableListOf<View>()
    private var selected = -1

    init {
        orientation = VERTICAL
        layoutParams = ViewGroup.LayoutParams(LayoutParams.MATCH_PARENT, LayoutParams.MATCH_PARENT)
        addView(contentHost, LayoutParams(LayoutParams.MATCH_PARENT, 0, 1f))
        val divider = View(context)
        divider.setBackgroundColor(0xFFE0E0E0.toInt())
        addView(divider, LayoutParams(LayoutParams.MATCH_PARENT, dp(1).coerceAtLeast(1)))
        titles.orientation = HORIZONTAL
        titles.setBackgroundColor(0xFFF8F8F8.toInt())
        addView(titles, LayoutParams(LayoutParams.MATCH_PARENT, LayoutParams.WRAP_CONTENT))
    }

    private fun dp(value: Int) = (value * resources.displayMetrics.density).toInt()

    fun addTab(label: String, content: View) {
        // Do not create cycles or reuse the same view for two tabs.
        var ancestor: View? = this
        while (ancestor != null) {
            if (ancestor === content) return
            ancestor = ancestor.parent as? View
        }
        if (contents.contains(content)) return
        (content.parent as? ViewGroup)?.removeView(content)
        content.visibility = View.GONE
        contentHost.addView(content, FrameLayout.LayoutParams(
            LayoutParams.MATCH_PARENT, LayoutParams.MATCH_PARENT))
        val index = contents.size
        contents.add(content)
        val title = TextView(context)
        title.text = label
        title.textSize = 14f
        title.gravity = Gravity.CENTER
        title.setPadding(dp(12), dp(8), dp(12), dp(8))
        title.setTextColor(0xFF6B7280.toInt())
        title.setOnClickListener {
            selectTab(index)
            PerryBridge.nativeInvokeCallback1(callbackKey, index.toDouble())
        }
        titles.addView(title, LayoutParams(0, LayoutParams.WRAP_CONTENT, 1f))
        if (selected == -1) selectTab(0)
    }

    /** Programmatic selection is silent; taps invoke onSelect after the content changes. */
    fun selectTab(index: Int) {
        if (index !in contents.indices) return
        selected = index
        contents.forEachIndexed { i, content ->
            content.visibility = if (i == index) View.VISIBLE else View.GONE
            val title = titles.getChildAt(i) as TextView
            title.isSelected = i == index
            title.setTextColor(if (i == index) 0xFF2563EB.toInt() else 0xFF6B7280.toInt())
        }
    }
}
