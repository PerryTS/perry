Per-unit facts: a function value a unit holds as its own parameter and calls
as a method of one of its own objects (`o.k()` with the parameter under `k`)
now reaches the caller's unit with that object described as the call's
`this`, the same way the call's arguments travel (DECISIONS 140). Before, the
method-call path reached the parameter marker through the object's field and
neither recorded the call nor escaped the receiver, so the unit that owned the
object never learned that another unit's function ran on it: the owner kept
`n` readonly and numeric while the function wrote a string into it, and a
build with PERRY_VERIFY_FACTS=1 aborted on the false proof. The caller's unit
runs its function on a stand-in of the owner's layout; what it does to the
stand-in is its report, which the owner takes as for any contract value (a
broken literal is undescribed and escapes, a broken class is no contract, a
store into an existing field comes back as a store effect). Where the layout
leaves with the function value under a key any reader may name, the field is
sunk and the marker escapes with it: the call is then one of an unknown
function value, and the receiver escapes as whole-program facts escape the
receiver of such a call.

Tests: perry-wpfacts two- and three-unit tests of the fixture (per unit never
stronger than whole, the owner's copy writable, the receiver escaped), the
private-field variant (the receiver described, the owner's display broken),
the broken-layout variant (no stable class), and a linked program under both
facts modes with PERRY_VERIFY_FACTS=1 compared against node.
