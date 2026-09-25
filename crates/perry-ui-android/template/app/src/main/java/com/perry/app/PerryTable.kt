package com.perry.app

import android.content.Context
import android.graphics.Typeface
import android.os.Handler
import android.os.Looper
import android.view.View
import android.view.ViewGroup
import android.widget.FrameLayout
import android.widget.HorizontalScrollView
import android.widget.LinearLayout
import android.widget.ScrollView
import android.widget.TextView
import java.util.concurrent.FutureTask

/** A scrollable widget table. Cell callbacks return ordinary Perry widget views. */
class PerryTable(context: Context, rows: Int, private val columns: Int,
                 private val renderKey: Long) : HorizontalScrollView(context) {
    companion object {
        fun <T> onUi(action: () -> T): T {
            if (Looper.myLooper() == Looper.getMainLooper()) return action()
            val task = FutureTask<T> { action() }
            Handler(Looper.getMainLooper()).post(task)
            return task.get()
        }
    }
    private val grid = LinearLayout(context)
    private val header = LinearLayout(context)
    private val body = LinearLayout(context)
    private val widths = IntArray(columns) { dp(120) }
    private val selected = sortedSetOf<Int>()
    private var multiple = false
    private var selectionKey = 0L
    private var sortKey = 0L
    private var sortColumn = -1
    private var ascending = true
    private var filterText = ""

    init {
        isFillViewport = true
        layoutParams = ViewGroup.LayoutParams(-1, -1)
        grid.orientation = LinearLayout.VERTICAL
        header.orientation = LinearLayout.HORIZONTAL
        body.orientation = LinearLayout.VERTICAL
        header.setBackgroundColor(0xFFF3F4F6.toInt())
        for (col in 0 until columns) {
            val title = TextView(context)
            title.setTypeface(null, Typeface.BOLD)
            title.setPadding(dp(8), dp(8), dp(8), dp(8))
            title.setOnClickListener {
                if (sortKey != 0L) {
                    ascending = if (sortColumn == col) !ascending else true
                    sortColumn = col
                    PerryBridge.nativeInvokeCallback2(sortKey, col.toDouble(), if (ascending) 1.0 else 0.0)
                }
            }
            header.addView(title, LinearLayout.LayoutParams(widths[col], -1))
        }
        grid.addView(header, LinearLayout.LayoutParams(-1, -2))
        val scroll = ScrollView(context)
        scroll.addView(body, ViewGroup.LayoutParams(-1, -2))
        grid.addView(scroll, LinearLayout.LayoutParams(-1, 0, 1f))
        addView(grid, FrameLayout.LayoutParams(-2, -1))
        rebuild(rows)
    }
    private fun dp(value: Int) = (value * resources.displayMetrics.density).toInt()
    private fun paintSelection() {
        for (row in 0 until body.childCount)
            body.getChildAt(row).setBackgroundColor(if (row in selected) 0xFFDBEAFE.toInt() else 0x00000000)
    }
    private fun choose(row: Int) {
        if (multiple) { if (!selected.add(row)) selected.remove(row) }
        else { selected.clear(); selected.add(row) }
        paintSelection()
        if (selectionKey != 0L)
            PerryBridge.nativeInvokeCallback1(selectionKey, (selected.firstOrNull() ?: -1).toDouble())
    }
    private fun rebuild(count: Int) {
        body.removeAllViews()
        selected.removeAll { it >= count }
        for (row in 0 until count) {
            val line = LinearLayout(context)
            line.orientation = LinearLayout.HORIZONTAL
            line.minimumHeight = dp(36)
            line.setOnClickListener { choose(row) }
            for (col in 0 until columns) {
                val cell = FrameLayout(context)
                cell.setPadding(dp(8), dp(6), dp(8), dp(6))
                cell.setOnClickListener { choose(row) }
                val view = PerryBridge.nativeTableRenderCell(renderKey, row, col)
                if (view != null) {
                    (view.parent as? ViewGroup)?.removeView(view)
                    cell.addView(view, FrameLayout.LayoutParams(-1, -2))
                }
                line.addView(cell, LinearLayout.LayoutParams(widths[col], -1))
            }
            body.addView(line, LinearLayout.LayoutParams(-1, -2))
        }
        paintSelection()
    }
    fun setHeader(col: Int, title: String) = onUi {
        if (col in 0 until columns) (header.getChildAt(col) as TextView).text = title
    }
    fun setColumnWidth(col: Int, width: Double) = onUi {
        if (col in 0 until columns && width.isFinite() && width >= 0) {
            val px = (width * resources.displayMetrics.density).toInt()
            widths[col] = px
            val cells = mutableListOf(header.getChildAt(col))
            for (row in 0 until body.childCount)
                cells.add((body.getChildAt(row) as ViewGroup).getChildAt(col))
            for (cell in cells) { cell.layoutParams.width = px; cell.requestLayout() }
        }
    }
    fun updateRows(count: Int) = onUi { rebuild(count.coerceAtLeast(0)) }
    fun setSelectionCallback(key: Long) = onUi { selectionKey = key }
    fun setSortCallback(key: Long) = onUi { sortKey = key }
    fun selectedRow(): Int = onUi { selected.firstOrNull() ?: -1 }
    fun setMultiple(allow: Boolean) = onUi {
        multiple = allow
        if (!allow && selected.size > 1) {
            val first = selected.first(); selected.clear(); selected.add(first); paintSelection()
        }
    }
    fun selectedCount(): Int = onUi { selected.size }
    fun selectedAt(index: Int): Int = onUi { selected.elementAtOrNull(index) ?: -1 }
    // Like macOS, filtering is passive: callers update their data and row count.
    fun setFilter(text: String) = onUi { filterText = text }
    fun getFilter(): String = onUi { filterText }
}
