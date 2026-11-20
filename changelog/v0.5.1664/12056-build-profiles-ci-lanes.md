Perry now has a fast, debuggable `dev` profile and an optimized `prod` profile
that uses parallel code generation while retaining ThinLTO. Local build targets
and CI test lanes use the matching profiles, and the test guide now documents
fast, mid, and slow Rust suites with explicit timeboxes.
