Raise the net loop's handle ceiling from 4,096 to 65,536, taking turnloop
0.1.0-alpha.5.

A turnloop-backed server refused the 2,049th concurrent connection (#10351): a
connection costs two handles, and the refusal was a flat error rather than
backpressure. Sequential connects stopped at exactly 2,048, ruling out any
listener-backlog effect.

The number was small because turnloop *allocated* it. `Table::new` built every
slot and the whole free list up front, and nine other structures were sized the
same way — measured at 1,249 bytes and 2 allocations per handle of ceiling — so
the limit was paid whether or not it was used. Raising it would have cost real
memory on every loop, and Perry had just moved to one loop per JS agent.

turnloop 0.1.0-alpha.5 pages those tables (turnloop#75). A loop's idle cost is
now one page and is byte-identical from 1K to 1M handles: 784 allocations and
5,000 KB at any ceiling, against 151,867 allocations and 95.7 MB at 65,536
before. Only the high-water mark costs anything. alpha.5 also reserves an
accept's handle slot at submission, so a full table applies backpressure
instead of letting the kernel hand over a connection the driver then destroys.

The ceiling being free is the point: 65,536 handles is ~32,768 connections, and
raising it further is now a one-line change with no idle cost.
