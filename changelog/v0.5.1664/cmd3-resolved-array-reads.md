Generic array-like indexed reads consume the array head they already resolved
instead of entering the polymorphic array getter and repeating receiver checks.
Both entry points share the same indexed Get implementation, including accessors,
sparse elements, holes and prototype reads; no cache or alternate read algorithm
is added.

Experimental perf result on qb6: commander with auto optimization uses 0.76%
fewer user instructions (3.19x Node), with 64 bytes more emitted text, 32 KiB
less THP-off peak RSS, and unchanged 0 full / 20 minor collections. This does
not meet the lane's 10% landing threshold or the 2x Node goal. The shared body
is outlined on the full-runtime build; its extra transfer explains small
instruction increases on other array-read-heavy programs.
