Skip the by-name property's typed-array, Date, Temporal and URLSearchParams probes
when the receiver's existing shape kind proves it is ordinary. Keep the native
reads and conservative fallback for unmarked shapes on the shared read path,
with unit coverage for skipped probe work and a Node parity gap test covering
ordinary objects and native receivers.
