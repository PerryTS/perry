Program facts are solved over the whole program: one solve over every
module, where a function body contributes only once the program can run
it. The per-unit solve (one solve per module, contracts exchanged between
the units to a fixed point) is retired; `PERRY_FACTS_UNITS=1` keeps it
reachable for its tests until it is deleted. On tsc-sized programs the
facts pass is about 10x faster (the TypeScript compiler's sources: 652 s
per unit, 32 s whole, on the same host) and proves more sites (22,792
against 20,644); small programs lose only proofs in functions the program
never runs.

The flip came with five soundness rules, each with a fixture under
`test-files/test_gap_*`: a class whose `extends` value is a native
constructor or a function born by one gives no fixed-layout or
prefix-stable template below it; a `super()` call is static only when the
evaluated parent is the linked parent, and the fact names that parent; a
class expression evaluated more than once gives no fact keyed on its class
object or private brand; a per-evaluation template reserves its birth
keys instead of owning them; and a static family lists its constructor
keys in the class reservation order the facts and the key index read.
