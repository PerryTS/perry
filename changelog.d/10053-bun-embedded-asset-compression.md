Compress worthwhile embedded payloads in Bun-platform executables with
checksummed zstd,
using the existing Bun CLI runtime feature. Keep small/incompressible payloads
raw and require at least 256 KiB of aggregate payload savings before enabling
compressed registration. Generated constructors validate and decode one exact
frame into immortal native storage before registration, preserving paths,
lengths, empty/binary bytes and explicit text-loader metadata. Corrupt data or
length/loader mismatches fail before publishing an asset. Existing raw APIs and
non-Bun embedding remain unchanged. Independent decoder, packaging, and native
filesystem/Bun/text-loader tests cover the compressed path and raw controls.
