Make macOS `setPadding` work on buttons, Text labels, and AttributedText labels,
including intrinsic sizing and asymmetric content placement. Preserve native
button cells, styling, and target/action wiring, and invalidate layout when
padding changes. Respect flipped AppKit coordinates for text-field insets.
Unsupported native views now produce a diagnostic in development builds; wrap
them in a padded stack. Add native size and rendered-content regression tests.
