Fix `.length` on short inline strings containing non-ASCII text. Runtime
property reads, suffix cursors, typed and generic generated reads, and
element-shape loop clones now count UTF-16 code units instead of UTF-8 bytes.
Non-ASCII strings remain eligible for inline storage; the generated ASCII
path uses the stored byte count, and the Unicode path stays call-free.
