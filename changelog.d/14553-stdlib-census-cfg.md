The `rustc warnings` CI lane is one step closer to green: the five crypto
randomness entry points no longer compile an unreachable uninstrumented tail on
the linux-x86_64 instrumented census build. `census_inputs!` returns through the
instrumented census there while the plain tail below it carried no complementary
`cfg`, so `-D warnings` flagged the whole tail dead; each tail is now wrapped in
`cfg(not(all(perry_gc_instruments, linux, x86_64)))`, one branch per
configuration. Behavior is unchanged on every platform, including macOS, where the instrumented branch never exists.
