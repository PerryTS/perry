Fixed `PERRY_NO_AUTO_OPTIMIZE=1` builds that import `node:http` linking archives
from two different cargo builds (follow-up to #11225).

Importing `http` makes the no-auto path rebuild the stdlib with
`external-http-client-pump`. That rebuild built only `perry-stdlib-static` and
`perry-ext-http`. The runtime archive and every other wrapper the program
links (`perry-ext-net`, `perry-ext-events`, ...) stayed prebuilt, built by a
different cargo invocation with a different feature unification. The binary
therefore carried two copies of perry-ffi, each with its own statics:

- Two handle-id pools, so stdlib and ext-net ids aliased again. #11225 had
  fixed that only for a single perry-ffi.
- The raw-net vtable that ext-net publishes was invisible to ext-http.
- ext-net itself was linked twice.
- A runtime split from the one bundled into the stdlib archive. `mi_free`
  crashed on the first regex match (#11240, first fixed by #11226).

The rebuild now selects `perry-runtime-static` and every well-known wrapper the
program links in the same cargo invocation, and replaces every prebuilt copy
with the rebuilt one. A no-auto binary with `http` now carries one perry-ffi,
one perry-runtime and one copy of each wrapper.

Integration test: `no_auto_http_rebuild_builds_every_linked_archive_in_one_graph`
runs the real cargo command on a miniature workspace. That workspace's
ext-net only compiles when perry-ffi is unified with the stdlib's pump
feature, and decoy prebuilt archives must not be linked.
