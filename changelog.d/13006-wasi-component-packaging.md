Include the optional native Component Model host archive and `perry-wit` in
release payloads for macOS ARM64, Linux x86_64 GNU and Windows x86_64. Preserve
them through npm staging, Homebrew and the shell installer; add an optional
Debian `perry-components` package tied to the exact compiler version. npm gains
a `perry-wit` launcher with argument, stdin and exit-status propagation.

Require the complete tool/archive pair and qualify the final archive outside
the checkout, including WIT generation, missing-engine rejection, ordinary
programs without the engine, and real typed component calls, resources and
TCP/UDP/DNS interoperability with the engine.
Keep runtime GC-effect sidecars in the installation. Execution qualification
of the release profile is pending; no release is published by this change.

Qualify the merged ABI 5 engine from the final archive, requiring all 20 normal
library phases and five command phases, including host resources and commands
nested in active library callbacks. Retain command evidence and compare exact
TCP/UDP payloads. Include the loop-interrupt IR NUL correction exposed by Windows
host qualification so packaged compilers can reach execution validation.

Include the Windows linked-image boundary and COFF helper corrections from
#13004. Initial production Linux/macOS archives passed the earlier 15-phase
profile. Measure the actual Linux production engine at 235,912,106 bytes raw and
39,653,474 bytes compressed in npm staging; pack/unpack, WIT generation, automatic
engine discovery and all 51 typed fixture calls pass. Expanded ABI 5 production
qualification remains separate and pending.

Native Windows installs now discover runtime, stdlib and component archives beside
`perry.exe` and in npm's sibling `lib` directory, including the implicit host
target. Cross-architecture compilation keeps target-specific lookup. The release
qualification runs both Windows lookup regressions before testing the isolated
archive, and retains their log with the packaging evidence.
