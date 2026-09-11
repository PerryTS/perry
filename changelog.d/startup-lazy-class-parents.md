Defer each application image's 256 KiB dense class-parent table until its first
representable inheritance edge is registered. Empty programs and images without
inheritance avoid the allocation. Parent lookups, publication ordering, shared
worker images, and isolated application images retain their existing behavior.
