# Perry Doc Examples

Every `.ts` file under this directory is a real, compilable program that is
verified by `cargo run -p perry-doc-tests` in the full doc-tests CI job.
Documentation pages in `docs/src/` pull these files in via mdBook's
`{{#include}}` directive, so the
code you see on the rendered docs site is the same code CI is checking.

## Adding an example

1. Create a `.ts` file under the appropriate subfolder (`runtime/`, `ui/`, etc.).
2. Start the file with a three-line banner:

```ts
// demonstrates: <one-line description>
// docs: docs/src/<page.md> (optional — where the example is referenced)
// platforms: macos, linux, windows
```

Runtime examples (non-UI) should list all three platforms. UI examples list
whichever platforms their widgets support. The harness skips an example whose
banner doesn't include the current host platform.

Examples that need specialized runtime libraries, such as Fastify's request
pump, add `// requires: auto-optimize` to the opening banner (within its first
15 lines). The harness removes `PERRY_NO_AUTO_OPTIMIZE` from that example's
compiler process, so it builds the required libraries and participates in the
normal pass/fail report. Other examples keep using the host runner's prebuilt
libraries. Unknown requirements fail discovery rather than silently being
ignored.

Use `// run: false` for examples that need external services or run indefinitely.
They still compile and link; combining it with `requires: auto-optimize` tests
the specialized build without starting a server or connecting to a database.

3. Reference it from markdown:

```markdown
\`\`\`typescript
{{#include ../../examples/ui/counter.ts}}
\`\`\`
```

4. (Optional) For runtime examples with deterministic stdout, create a
   matching expected-output file at
   `_expected/<same/relative/path>.stdout`. The harness will byte-compare
   actual stdout against it after trimming trailing whitespace.

## Subfolder layout

| Folder | Content |
|---|---|
| `runtime/` | Language and stdlib examples (non-UI). |
| `ui/` | UI examples that open a window. Run with `PERRY_UI_TEST_MODE=1`. |
| `_expected/` | Golden stdout files for runtime examples. |
| `_baselines/` | Per-platform screenshot baselines (added in the gallery PR). |
| `_harness/` | Shared bootstrap/helper files, not discovered as tests. |

## Running locally

```bash
# All examples
./scripts/run_doc_tests.sh            # macOS / Linux
pwsh ./scripts/run_doc_tests.ps1      # Windows

# Just one
cargo run -p perry-doc-tests --release -- --filter ui/counter.ts --verbose

# Screenshot baseline: write/overwrite for the current host OS
cargo run -p perry-doc-tests --release -- --filter ui/gallery.ts --bless
```

Binaries are built into `target/perry-doc-tests/`. A JSON summary can be
written with `--json path/to/report.json`.

## Blessing a per-OS baseline from CI

Linux and Windows baselines can't be captured from a macOS dev box. Flow for
bootstrapping them:

1. Open a PR; the `doc-tests` matrix job runs on all three OSes. Gallery is
   advisory on any OS whose baseline isn't in `_baselines/<os>/gallery.png`
   yet — the run will surface `SCREENSHOT_DIFF: no baseline at ...`.
2. Download the `gallery-screenshots-<os>` artifact from the CI run. It
   contains the freshly captured `gallery_<os>.png`.
3. Rename to `gallery.png` and commit it at
   `docs/examples/_baselines/<os>/gallery.png`.
4. Re-run the job; the advisory gate will now produce a real SSIM score. Once
   two consecutive runs fall well under the threshold, flip
   `gallery_advisory: true → false` for that OS in
   `.github/workflows/test.yml`.
