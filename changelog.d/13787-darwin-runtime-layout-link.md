### Fixed

- Admit only catalogued optional runtime-layout symbols when linking Darwin executables, so sparse compiler-owned descriptors can remain absent weak references without relaxing unrelated undefined-symbol errors. Preserve driver, direct-linker and Swift argument forms.
