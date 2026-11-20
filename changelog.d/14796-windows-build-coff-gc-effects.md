Windows: `perry-runtime` builds again (`snap_image` read `/proc/self/maps`
through the Unix-only `OsStrExt`), and the GC-effects sidecar for the Windows
archives finishes in seconds instead of running for hours past 14 GB: COFF
names every COMDAT section of a kind alike (`.rdata`), so a relocation through
a section symbol reached every same-named section of its object. COFF
sections are now named by index (`name#index`); ELF and Mach-O are unchanged.
