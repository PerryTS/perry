The public entry of a function with `number` parameters now tests each Number
argument with an ordered-compare branch first (`ucomisd; jnp` on x86-64): every
boxed tag is a NaN pattern, so an ordered value is a plain number. Only
NaN-patterned arguments reach the second block, which runs the sign-closed
plain-number test (excluding HANDLE_TAG) and the int32-box test. The routing is
unchanged; the common path is two instructions instead of the sign fold's five.
