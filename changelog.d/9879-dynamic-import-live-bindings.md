Dynamic imports now expose aliased local `var`, `let`, and `const` exports
instead of resolving them as `undefined`. Namespace reads also preserve live
bindings when an exported mutable variable is reassigned after import.
