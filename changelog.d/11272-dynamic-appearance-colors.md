Add macOS light/dark RGBA setters for widget backgrounds and borders, text and
button titles, and button content tint. Dynamic NSColors follow effective
appearance overrides and high-contrast variants; an AppKit appearance observer
refreshes resolved layer colors. Static setters replace dynamic palettes, and
hide/show preserves them. Includes native appearance/lifecycle regression
coverage, public dispatch and TypeScript declarations, and a macOS example.
