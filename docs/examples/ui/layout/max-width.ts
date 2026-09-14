// demonstrates: responsive centered content with a maximum width and padding
// docs: docs/src/ui/layout.md
// platforms: macos, linux, windows
// targets: web, wasm

import { App, VStack, Text, widgetSetMaxWidth, setPadding } from "perry/ui";

const column = VStack(12, [Text("Responsive content")]);
setPadding(column, 32);
widgetSetMaxWidth(column, 720);
App({ title: "Content", width: 1000, height: 600, body: VStack(0, [column]) });
