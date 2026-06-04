// Issue #3621 — textSetTextAlignment smoke test.
// Builds a column of Text widgets, one per alignment value, and applies
// textSetTextAlignment to each. Renders a real window on macOS; the call
// must link against perry_ui_text_set_text_alignment on every backend.
import {
  App, VStack, Text, widgetSetWidth,
  textSetTextAlignment, textSetFontSize,
} from "perry/ui"

const left = Text("Left aligned (0)")
textSetTextAlignment(left, 0)

const right = Text("Right aligned (1)")
textSetTextAlignment(right, 1)

const center = Text("Center aligned (2)")
textSetTextAlignment(center, 2)

const justified = Text("Justified text (3) — a longer line so wrapping shows the fill")
textSetTextAlignment(justified, 3)

const natural = Text("Natural / locale (4)")
textSetTextAlignment(natural, 4)

const rows = [left, right, center, justified, natural]
for (const r of rows) {
  textSetFontSize(r, 16)
  widgetSetWidth(r, 360)
}

const body = VStack(12, rows)
widgetSetWidth(body, 380)

App({ title: "Text Alignment (#3621)", width: 400, height: 320, body })
