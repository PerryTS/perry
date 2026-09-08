Release candidates now build every governed native extension in one Cargo
invocation with the compiler, runtime, and stdlib archives. Packaging fails if
an extension archive is missing or if any Tokio-using extension contains a
different Tokio instance from the stdlib; CPU-only extensions remain valid.
The Linux glibc and musl images also use a dated Bullseye security snapshot so
the release build does not depend on packages already removed from Debian's
live security mirror.
