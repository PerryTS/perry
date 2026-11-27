The `Array.fromAsync` GC test helper reads string bytes through the runtime accessor instead of an open-coded header offset, so the string payload-access inventory is back at its baseline.
