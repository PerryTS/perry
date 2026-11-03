Keep a typed array alive through a specialized function call even when preparing
that call is the caller's last use of the array. The raw-pointer calling
convention still avoids repeated type checks, while native GC roots retain the
owner until the callee returns. This fixes collected typed-array storage and
incorrect checksums in all five full-collection representation stress arms.
