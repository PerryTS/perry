Symbol accessors now use the same private AccessorPair storage and decoder
as string accessors. This removes their separate two-word representation,
two public array pushes at installation, and public array getter dispatch
when resolving an accessor or an iterator method. The property's shape
attribute remains the authority for interpreting its value slot.

Add Node parity coverage for own and inherited accessors on ordinary
objects, arrays, and functions, descriptor changes, deletion, and accessor
iterator methods. The copying-GC test verifies that the shared pair and
its closure remain reachable through the moved owner.

The symbol-accessor micro drops from 23,123 to 20,410 instructions per
iteration (11.7%). Fastify removes 13 public array-getter entries per inject.
Together with the canonical-key hashing change, steady callgrind cost drops
from 1,268,143 to 1,249,626 instructions per inject (1.46%). The full fastify
driver drops from 13.794 to 13.593 billion user instructions in an interleaved
n=5, ASLR-off, THP-off A/B; emitted text shrinks by 2,240 bytes. The generated
micro kernel remains 2,301 bytes, with the same five-byte call instructions.

All 19 program outputs and four class micros match Node 26.5.1. Instructions
are flat or lower across the program set; the class per-iteration costs stay
flat. Runtime and codegen library suites reproduce their exact baseline
failure sets (14 each), with no new failures. The copying-GC deletion negative
control fails as intended. Linux GC effects match all 4,339 archived symbols;
macOS and Windows tables were not regenerated.
