Property stores on an object whose prototype resolves to itself (an fs stream)
stop after one repeated prototype instead of walking 64 hops. Chaining 1500
read and write streams took 17 s and now takes about 4 s.
