Regenerated the macOS and Windows GC call-effects tables. Nine runtime symbols
had no row, and two had changed from `ThrowOnly` to `Leaf`, so codegen treated
those calls more conservatively than it needed to.
