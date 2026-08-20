Fixed a moving-GC safety hazard in `punycode.ucs2.decode` by copying string
payload bytes before decoding them instead of exposing the heap payload through
an unbounded borrowed slice.
