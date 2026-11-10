// Issue #1495 — Image({ systemName }) SF-symbol object form.
//
// The SF-symbol image widget already exists as ImageSymbol(name); this
// wires the more ergonomic object-literal form Image({ systemName }) to
// the same runtime (perry_ui_image_create_symbol), matching the existing
// Image({ url, alt }) overload.
//
// Compile-smoke only (perry/ui renders a native window). Verified via the
// emitted LLVM IR: Image({ systemName }) lowers to perry_ui_image_create_symbol,
// identical to ImageSymbol(name); Image({ url }) still lowers to
// perry_ui_image_create_url.
import { App, VStack, Image, ImageSymbol } from "perry/ui";

App({
  title: "Symbol Test",
  width: 300,
  height: 200,
  body: VStack([
    Image({ systemName: "gear" }),
    Image({ systemName: "star.fill" }),
    ImageSymbol("bell"),
    Image({ url: "https://example.com/a.png", alt: "A" }),
  ]),
});
