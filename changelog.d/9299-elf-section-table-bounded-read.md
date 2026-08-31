Locating the GC-map section on Linux read the **entire image**. `dl_iterate_phdr`'s
callback did `std::fs::read(path)` — for the executable, `/proc/self/exe` — purely
to parse the ELF section-header table, and because `/proc` reports a size of 0
`read_to_end` doubled its buffer all the way to the image's real length.

Measured on claude-code (`cli_2.1.112.js`, 347 MB symbolized), isolated with
`PERRY_LAZY_STACK_MAPS=0` on `--version` (which never collects, so the flag turns
the whole GC-map initialization on and off inside one binary): **42.8 ms across 23
`read` calls**, plus the `memmove` traffic of the doublings and a transient RSS
spike large enough that it, not the 117 MB index, dominated the +321 MB peak I had
been attributing to the index. That is roughly **half** the cost of GC-map
initialization, and none of it is the GC map.

The walk now opens the image and `pread`s the three ranges it actually needs — the
64-byte ELF header, the section-header table, and the section-name string table —
a few kilobytes instead of a few hundred megabytes. `Elf64_Shdr`-sized entries and
finite table sizes are required rather than assumed, because `e_shnum` and
`e_shentsize` are `u16`s whose product can name 4 GB.

Two behaviours are deliberately preserved and one is deliberately changed:

- An image that cannot be **opened** is still reported, so `build_stack_map_index`
  refuses to publish an index that may be missing one image's native roots. A file
  that is merely not a usable ELF64 — truncated, or not ELF at all — is still
  skipped, exactly as a failed parse of its fully-read bytes was. A genuine I/O
  error now joins the first group rather than the second.
- The name table is bounded by its **own** `sh_size` instead of by the end of the
  file, so a `sh_name` pointing past it can no longer match bytes belonging to some
  other section.
- That bound makes an unresolvable `sh_name` reachable for the first time — with
  ~350 MB of slack after the table, a short name near its end always resolved — so
  such an entry is now **skipped rather than ending the search**. Ending it would
  drop `.perry_gcmap` in any image that lays a short name out before it, and the
  collector would then find no native roots with no diagnostic at all.

Linux only: the Mach-O and PE loaders walk structures the loader has already
mapped and never open a file.
