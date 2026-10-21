Add a source-matched WASI runtime bundle to the release pipeline and enable
WASI compilation in Linux x86_64 GNU, macOS ARM64 and Windows x64 release
compilers. Their archives place the runtime beside the executable for automatic
discovery. WASI SDK 34 remains a separate host prerequisite.

The runtime bundle records its source revision, Perry version, target, size and
checksum. Packaging verifies that identity and runs the smoke, acceptance and
pinned-Node comparison suites with the compiler installed outside its checkout.
A missing-runtime check prevents an ambient archive from masking a packaging
failure. Republishing older tags preserves their original feature profile.
