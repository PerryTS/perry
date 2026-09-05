**`PERRY_CONCAT_SITE_CACHE` is now a build-cache input.** #9514's per-site
concat cache reads the variable as a build-time kill switch — setting it to
`0` removes the lowering lane entirely — but it was registered neither in
`BUILD_CACHE_ENV_VARS` nor as a justified exclusion. A build with the switch
flipped could therefore be served a cached object produced with it in the
other state.

`codegen_env_vars_are_build_cache_inputs` caught this by scanning
`crates/perry-codegen/src` for every `env::var("PERRY_…")`, which is why the
check scans the source instead of trusting a hand-maintained list.
