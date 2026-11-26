The JSON parser and raw-json GC root scanners are registered once per thread, and the prototype-words prune runs only in full collections.
