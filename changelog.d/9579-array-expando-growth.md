Named properties attached to arrays remain visible after an indexed write grows
the array's backing allocation.

The named-property table is keyed by the array header address, while growth
replaces that header and leaves a forwarding stub behind. Direct runtime
coverage now pins the required owner transfer to the replacement header and
asserts that the stale owner is removed. A Node differential also covers typed
and dynamic arrays through growth, including property enumeration and JSON
serialization.
