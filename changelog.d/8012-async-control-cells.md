### Faster compiler-private async control cells (#8008)

Async and generator state machines now access their compiler-minted state,
pending-type, done, and executing cells with direct typed loads and stores.
Those cells are preallocated as single-field `I32Box` or `BoolBox` values, so
their pointer provenance and representation are already proven; routing every
access through the general runtime helpers redundantly checked the same pointer
against a thread-local box registry.

Ordinary user captures keep the checked runtime path, including its invalid-box
protection. Primitive allocation and registry insertion also remain unchanged;
only access to the four private controls bypasses repeated validation.

On the refreshed async probe set this removes 7.08% of retired instructions
from the pure async/await topology, 5.92% when plain objects flow through the
same topology, and 2.10% from the full `asyncpipe` workload. The synchronous
and Promise.all-only controls remain flat. An IR regression locks down both
the narrow eligibility boundary and the direct typed accesses.
