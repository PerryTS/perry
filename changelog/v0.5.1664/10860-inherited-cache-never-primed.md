Fixed two defects that made #10834's inherited-read cache **pure overhead** —
the probe ran on every read, never served, and the chain walk proceeded
unchanged. Shipped in v0.5.1626 and measurably slower than before it.

**A. The prime site was gated on the wrong miss reason.** `get_field_ic_miss_impl`
primed only under `R::NotOwn`. A receiver with no keys array reports
`ObjectNoKeys` and returns from an earlier arm, several hundred lines before the
prime — and `Object.create(p)` with nothing of its own is exactly that shape,
the most common inherited-read receiver there is. `ObjectNoKeys` means the
object has no own properties at all, so the prime's precondition holds there
*more* strongly than under `NotOwn`.

**B. The slot index ignored the class id.** `js_object_create` mints a fresh
synthetic class id per call, so N receivers from `Object.create(p)` have N class
ids and one shape. `entry_index` hashed only (shape, key), so all N shared a
slot and evicted each other, while the entry compare on `recv_class_id` made
every read miss, re-walk and re-prime: `hits=0 primes=6295655` over ten million
reads.

Measured, `perf stat -e instructions:u`, min of 3, fitted 500 k → 5 M:

| fixture | before | after | node |
|---|---|---|---|
| 8 `Object.create` receivers via an array | 1600 | **494** | 19.1 |
| single keyless receiver, 1-level chain | 1481 | **427** | 9.0 |
| single keyless receiver, 3-level chain | **2442** | **446** | 8.7 |

The depth row is the one that matters: a keyless three-level chain paid 2442
instructions per read, because every read re-walked all three hops.

Counters after are `primes=1` / `primes=8` then hits — exactly one prime per
receiver.

**Why #10834's own measurement missed both.** Its fixtures gave the receiver an
own property and mutated it in the loop (`O.x = k`, added to keep node's
optimiser honest). That incidental detail put the read on the `NotOwn` path, so
defect A never fired, and used a single receiver, so defect B never fired. On
that one shape the cache genuinely was a 43% win — which is why the reported
numbers were real and generalised badly.
