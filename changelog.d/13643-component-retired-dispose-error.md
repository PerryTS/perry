When a guest trap retires a component's Store, the destructors of the host
payloads it still held run at retirement. Their first exception is now thrown
from the next `dispose()`, once, as legacy disposal does; later destructor
exceptions are suppressed. Before, the trap was delivered and the destructor
exception was discarded, because `dispose()` on a retired component returned
immediately. When the failing call has its own JavaScript exception (an import
that threw or rejected), that exception came first and stays the only one
reported, as before. The exception is kept in the component's one existing
root-failure slot; no table or cache is added.

The native `host-resources` component acceptance now has a second throwing
destructor (suppressed) and checks that a repeated `dispose()` throws nothing.
