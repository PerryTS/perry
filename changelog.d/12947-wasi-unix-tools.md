Extend the checksum-pinned WASI tool installer to ARM64 Linux and Intel/ARM64
macOS, alongside x86_64 Linux. Use the host SHA256 tool and preserve installation
paths containing spaces or apostrophes when exporting compiler flags. Import
the shell environment correctly in CI and add a macOS WASI build/execution leg.

Validate the ARM64 macOS download/extraction, cached path quoting, actual C-to-WASI
execution, and rejection of corrupted downloads. Verify all eight archive pins
against upstream release metadata and retain the x86_64 Linux cached-tool check.

Record 36 passing macOS execution checks for the previously validated
Linux-produced Perry components: 6 smoke, 20 acceptance, and 10 fresh comparisons
with Node 26.5.1. The committed receipt includes artifact and fixture hashes.
This establishes host execution; the newly configured macOS Perry build leg
still needs a successful CI run, and Intel macOS/ARM64 Linux remain unexecuted.
