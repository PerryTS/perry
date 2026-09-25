// demonstrates: Light/dark custom text, background, border and button tint colors.
// docs: docs/src/ui/styling.md
// platforms: macos
import {
    App, VStack, Text, Button, textSetDynamicColor,
    widgetSetDynamicBackgroundColor, widgetSetDynamicBorderColor,
    widgetSetBorderWidth, buttonSetImage, buttonSetDynamicContentTintColor,
} from "perry/ui";

const label = Text("Switch macOS appearance to change this palette");
textSetDynamicColor(label, 0.1, 0.2, 0.5, 1, 0.7, 0.8, 1, 1);
const button = Button("Favorite", () => {});
buttonSetImage(button, "star.fill", 18);
buttonSetDynamicContentTintColor(button, 0.5, 0.3, 0, 1, 1, 0.8, 0.3, 1);
const body = VStack(16, [label, button]);
widgetSetDynamicBackgroundColor(body, 0.95, 0.95, 1, 1, 0.08, 0.08, 0.15, 1);
widgetSetDynamicBorderColor(body, 0.3, 0.3, 0.6, 1, 0.6, 0.6, 0.9, 1);
widgetSetBorderWidth(body, 2);
App({ title: "Appearance colors", width: 480, height: 160, body });
