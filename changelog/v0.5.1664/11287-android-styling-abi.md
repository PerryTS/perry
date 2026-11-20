Fix Android styling setters reading widget handles from floating-point registers instead of the shared integer ABI. Backgrounds, gradients, borders, shadows, layout dimensions/insets, opacity, text fonts/colors and button styling now forward the correct handle and scalar arguments. Align the signatures of the existing text-spacing no-op exports as well.

Add ABI coverage for all 17 affected exports. Native argument-forwarding probes fail for all 15 implemented setters before the fix and pass afterward; the UI parity suite passes. No version bump.
