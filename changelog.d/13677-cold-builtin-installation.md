### Runtime

Apply size optimization to four builtin installation routines while hot helpers
retain the surrounding build profile. The O2/fat-LTO macOS ARM64 comparison saves
about 16 KB in linked applications at baseline steady-state performance. Include
reproducible size, instruction, cycle, startup and application measurements for
#13662. No side tables or caches are added.
