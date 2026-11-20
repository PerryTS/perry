Port GC effect analysis to the perry-gc-effects Rust library and CLI, reading
ELF, Mach-O and COFF archives directly through object and LLVM 22. Regeneration
uses the Rust tool, with an optional --cross-check against the retained Python
analyzer. The existing rules, hard checks and TSV bytes are preserved, with
three-format binary fixtures and planted-violation tests. Codegen, the compiler
pipeline and checked-in effect tables are unchanged.
