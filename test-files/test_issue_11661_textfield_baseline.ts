// Smoke test for issue #11661: on macOS a TextField must draw its text at the
// same height whether or not it is editing.
// Run the built app. The first field opens focused, so it draws through the
// field editor. The text in both fields must sit at the same height in its
// yellow box, and clicking into the second field must not move its text.
import { App, TextField, VStack, textSetFontFamily, textfieldSetBackgroundColor, textfieldSetBorderless, textfieldSetFontSize, textfieldSetString, textfieldSetTextColor, widgetSetBackgroundColor, widgetSetHeight } from "perry/ui"

function field(text: string, size: number) {
  const f = TextField("", () => {})
  textfieldSetBorderless(f, 1)
  textfieldSetFontSize(f, size)
  textSetFontFamily(f, "Helvetica")
  textfieldSetBackgroundColor(f, 1, 1, 0.7, 1)
  textfieldSetTextColor(f, 0, 0, 0, 1)
  widgetSetHeight(f, 40)
  textfieldSetString(f, text)
  return f
}
const body = VStack(20, [
  field("Hxg first 16pt", 16),
  field("Hxg second 16pt", 16),
  field("Hxg third 28pt", 28),
])
widgetSetBackgroundColor(body, 1, 1, 1, 1)
App({ title: "issue 11661 TextField baseline", width: 400, height: 240, windowState: "normal", body })
