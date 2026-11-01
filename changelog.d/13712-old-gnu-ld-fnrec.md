Programs link again with GNU ld before 2.36, such as binutils 2.35 on Debian
bullseye, the linker of the glibc 2.31 release floor. Since DCE layer 0
(#13636) the link failed there with "perry_fnrec has both ordered [...] and
unordered [...] sections" / "final link failed: bad value": each body's
metadata record is tied to its body (`SHF_LINK_ORDER`), but the image's zero
record (the sentinel that makes the section exist when every record is
dropped) was a plain section of the same name, and binutils before 2.36
rejects an output section that mixes the two. On ELF the sentinel is now tied
too, to the bounds descriptor, the one global that names it, so every
`perry_fnrec` input is link-ordered and the link keeps exactly one sentinel
per image on every ELF linker. GNU ld 2.46 and lld still drop dead bodies
exactly as before; GNU ld 2.35 ignores `-z start-stop-gc` and keeps the JS
bodies, which is larger but correct. Mach-O, COFF and wasm are unchanged. A test checks that
every `perry_fnrec` section in an emitted ELF object is `SHF_LINK_ORDER`.
