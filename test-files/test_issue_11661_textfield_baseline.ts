// Smoke test for issue #11661 and the issues that share its cause: macOS
// TextField, SecureField and Text build Perry's inset cell, which must keep the
// behaviour of the cell AppKit builds.
// Run the built app and check each row:
// - The first field opens focused, so the field editor draws it. Its text must
//   sit at the same height as the idle 16pt field below it (#11661).
// - Clicking into any field must not move its text (#11661).
// - The 28pt text must stay inside its yellow box (#11661).
// - The long value must stay on one line and scroll (#10155).
// - The padded field and the padded label must inset their text by 12 at the
//   top and 24 at the left (#9954, #10159).
// - The SecureField must show bullets on one line.
// - The label must be red (#10856).
import { App, SecureField, Text, TextField, VStack, setPadding, textSetColor, textSetFontFamily, textfieldSetBackgroundColor, textfieldSetBorderless, textfieldSetFontSize, textfieldSetString, textfieldSetTextColor, widgetSetBackgroundColor, widgetSetHeight } from "perry/ui"

function style(f: any, size: number, height: number) {
  textfieldSetBorderless(f, 1)
  textfieldSetFontSize(f, size)
  textSetFontFamily(f, "Helvetica")
  textfieldSetBackgroundColor(f, 1, 1, 0.7, 1)
  textfieldSetTextColor(f, 0, 0, 0, 1)
  widgetSetHeight(f, height)
}

function field(text: string, size: number) {
  const f = TextField("", () => {})
  style(f, size, 40)
  textfieldSetString(f, text)
  return f
}

const padded = field("Hxg padded 16pt", 16)
setPadding(padded, 12, 24, 0, 0)

const secure = SecureField("", () => {})
style(secure, 16, 30)
textfieldSetString(secure, "a long secret value that must stay on one line")

const label = Text("Red label padded 12 top, 24 left")
textSetColor(label, 1, 0, 0, 1)
setPadding(label, 12, 24, 0, 0)

const body = VStack(12, [
  field("Hxg first 16pt", 16),
  field("Hxg second 16pt", 16),
  field("Hxg third 28pt", 28),
  field("Hxg long value: assignee = currentUser() AND statusCategory != Done AND project = SU", 16),
  padded,
  secure,
  label,
])
widgetSetBackgroundColor(body, 1, 1, 1, 1)
App({ title: "issue 11661 TextField cell", width: 420, height: 420, windowState: "normal", body })
