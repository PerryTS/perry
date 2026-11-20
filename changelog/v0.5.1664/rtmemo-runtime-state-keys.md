Rust stream, EventEmitter, async-hooks and HTTP response reads and fixed-name
method calls now use
the existing StateKeyMemo through the emitted-read mechanism. Its shape
facts cover own slots, inherited holders and deep absence, with the existing
holder validation and GC roots. Deep runtime proofs use that site's existing
holder record and validate every hop, without a fixed chain-depth limit;
null-prototype absence is a receiver-shape fact. Existing writable-slot stores use the same
word; property creation keeps its existing Set path.

A whole-module invariant rejects raw generic named reads in these modules.
Tests cover deep-chain invalidation, spill storage, frozen and accessor
writes, inherited stream options and repeated once/emit.

The stacked lanes use one 56-byte holder entry for keyed reads, runtime
chain reads and method-chain priming. It owns the existing hop proof and
root scan, retains native-alias forwarding and ordinary-Get semantics, and
preserves the emitted method-chain offsets. Computed-key entries shrink
from 72 to 56 bytes; runtime depth remains usize without a fixed limit.

Keyed names and fixed-name identity-word addresses occupy the same word;
the existing NaN-box visitor rewrites keys and leaves metadata addresses alone.

Rebased integration retains the canonical Array/Map/Set primes in the existing MethodEntry and root scanner, alongside the shared 56-byte holder proofs. Bound-function shape kinds retain their ordinals; native aliases append kind 11. Declaration publication refreshes the anonymous-class role through the constant declaration path. GC call-effects tables are regenerated; the diagnostic native-stack-scan seed also names its already-seeded copying collector when optimization removes the standalone scan symbol.
