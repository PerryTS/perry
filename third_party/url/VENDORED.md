# url 2.5.9

This is the crates.io `url` 2.5.9 archive, SHA-256
`f00cd2407bb42c0ef17be88d4cf020f5cceffd121665bac9b8b9493a369b3b1a`
(rust-url commit `ad1c66cbff6e318393d872641e30589ff9687b9c`, see
`.cargo_vcs_info.json`). Licensed `MIT OR Apache-2.0`; `LICENSE-MIT` and
`LICENSE-APACHE` are the upstream files. The upstream Cargo.lock is omitted
because Perry resolves the dependency in its workspace lock. The workspace
`Cargo.toml` routes every `url` dependency of the graph here through
`[patch.crates-io]`; `idna`, `form_urlencoded` and `percent-encoding` still
come from crates.io.

url 2.5.9 is the latest release and implements older revisions of four steps of
the WHATWG URL Standard, so `new URL` differed from node (ada). The changes,
each marked `Perry:` in the source with the spec step it follows:

1. `src/parser.rs`, file host state (`parse_file_host`): an empty or parsed
   host continues in path start state, only a Windows drive letter buffer goes
   to path state. Upstream returned "has a host" instead, so an empty host took
   the drive-letter route. Path start state (`parse_path_start`) always writes
   the root slash; upstream skipped it when the serialization ended with `/`,
   which is `file://` with an empty host.
2. `src/parser.rs`, path state (`parse_path`): leading empty segments of a
   `file:` path are kept (`file:////x` -> `file:////x`); the spec dropped the
   step that removed them.
3. `src/parser.rs`, file state and file slash state (`parse_file`): against a
   `file:` base, url's host is always base's host, also when the input starts
   with a drive letter (`C|/` against `file://host/` -> `file://host/C:/`).
4. `src/parser.rs`, relative state and relative slash state
   (`parse_relative`): a special URL skips every further `/` and `\` after the
   second (`///test` against `http://example.org/` -> `http://test/`); a
   non-special URL goes to the authority only on `//`, and its `\` is a path
   code point.
5. `src/parser.rs`, opaque path state (`parse_cannot_be_a_base_path`): a space
   followed by `?` or `#` is written as `%20` (`data:space  ?q` ->
   `data:space %20?q`), and `src/lib.rs` no longer strips trailing spaces of an
   opaque path in `set_query(None)` / `set_fragment(None)` (the spec removed
   "potentially strip trailing spaces from an opaque path").

`tests/expected_failures.txt` drops the 58 WPT cases these changes make pass
(`cargo test` in a standalone copy: unit 71/71, WPT with no new failure);
`tests/unit.rs` updates `test_strip_trailing_spaces_from_opaque_path` to the
current spec and adds `perry_whatwg_file_and_opaque_path_steps` with the
expected strings from node 26. The cases still listed there are non-special
setter edges (`/.` prefix with the pathname and hostname setters, the port
setter with only tabs and newlines) and an href setter case where node agrees
with url.

Remove the vendor once an upstream release carries these steps and passes
Perry's normal dependency soak and compatibility checks.
