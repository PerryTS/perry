`performance.nodeTiming`'s getter is not a constructor, like every other built-in accessor, so `new` on the extracted getter throws a `TypeError`.
