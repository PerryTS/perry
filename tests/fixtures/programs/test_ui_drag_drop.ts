// Drag & drop API smoke test (issue #4773).
//
// Exercises the Phase-0 surface: the four widget-level drag/drop setters must
// compile through codegen (PERRY_UI_TABLE dispatch) and link against the
// platform backend's FFI symbols. On macOS the backend is currently a no-op,
// so this verifies the call path end-to-end (compile + link + run) rather than
// the native drag behavior itself.

import {
  App, VStack, Text,
  widgetOnDrop, widgetSetDragText, widgetSetDragFile, widgetSetDragUrl,
  type DropData,
} from "perry/ui"

// A drop destination: logs whatever payload is dropped onto it.
const dropZone = Text("Drop text / files / links here")
widgetOnDrop(dropZone, (data: DropData) => {
  if (data.text !== undefined) {
    console.log("dropped text:", data.text)
  }
  if (data.files !== undefined) {
    console.log("dropped files:", data.files.join(", "))
  }
  if (data.urls !== undefined) {
    console.log("dropped urls:", data.urls.join(", "))
  }
})

// A drag source offering three representations of the same drag.
const dragSource = Text("Drag me out")
widgetSetDragText(dragSource, () => "hello from perry")
widgetSetDragFile(dragSource, () => "/tmp/perry-drag-sample.txt")
widgetSetDragUrl(dragSource, () => "https://perryts.com")

console.log("drag/drop wired")

App({
  title: "Drag & Drop Test",
  width: 360,
  height: 200,
  body: VStack(16, [dropZone, dragSource]),
})
