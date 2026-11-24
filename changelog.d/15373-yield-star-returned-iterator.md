Fixed: `yield*` over an iterable whose `[Symbol.iterator]()` returns a separate iterator object no longer ends the generator when the delegate finishes; the statements after the `yield*` run.
