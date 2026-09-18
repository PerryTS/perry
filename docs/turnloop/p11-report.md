# turnloop P11 — the CLI, and the remaining HTTP client surfaces

Branch `turnloop/p11-cli-clients`, based on `turnloop/integration` at
`1edb5b7e8d` (P0–P8 plus `main` through v0.5.1579, version 0.5.1580). Built and
tested on the shared Linux build box (`perrybuilder`, EPYC 32c/64t) against the
pinned gap oracle Node **26.5.1** (`/opt/node-v26.5.1-linux-x64/bin`, not the
box default). Nothing here ran on Windows or macOS, and nothing here was
benchmarked — see "What was not run".

## The number, first

P8 made the migration countable. This is the first lane to move the count.

```
$ python3 scripts/tokio_inventory.py
tokio inventory: 39 manifest edges across 13 workspace crates,
20 tokio-family packages in Cargo.lock — unchanged.
```

**46 → 39 manifest edges, 16 → 13 workspace crates.** Seven edges and three
crates, and `scripts/tokio_inventory.json` is updated in the same commit — the
gate fails on a stale entry as well as on a new one, so the count is re-derived
from the tree rather than asserted here.

| P8 group | what it was | edges before | after |
|---|---|---|---|
| **J** — the `perry` CLI | `reqwest`, `tokio`, `tokio-tungstenite` | 3 | **0** |
| **G** — axios and node-fetch | `perry-ext-axios` ×2, `perry-ext-fetch` ×2, `perry-stdlib`'s `reqwest` | 5 | **1** |
| everything else | — | 38 | 38 |
| | | **46** | **39** |

Group G's survivor is `perry-stdlib`'s `reqwest`, which serves the declining
`fetch` path, `js_fetch_stream_start` and a proxied fetch. Those are P8's group
A and P6's stream surface, not this lane's.

**`Cargo.lock` is unchanged at 20 tokio-family packages**, and this report does
not claim otherwise: `perry-stdlib` still names every one of them.

## What this lane found before it changed anything

Two of the three subjects were already broken, and both were reproduced on the
**base commit** in this lane's own clone before anything was touched.

| subject | Node 26.5.1 | base `1edb5b7e8d` | P11 |
|---|---|---|---|
| `import 'node-fetch'` + a global `fetch()` | `status 200` | **SIGSEGV, 3/3 runs** | `status 200`, 3/3 |
| `import 'node-fetch'` alone, `r.status` | a number | **`TypeError: Cannot read properties of undefined (reading 'status')`**, 3/3 | a number, 3/3 |
| `axios` `r.statusText` on a 404 | `Not Found` | see "axios" below | — |

The first is P8's defect 1, which P8 also measured on `main` — so it is not a
turnloop regression, and it is the single most common network call in
JavaScript killing the process. The second is the observation P8 recorded and
did **not** diagnose. They have one cause, and this lane fixed it by deleting
the thing that caused it.

## `node-fetch`: the duplicate is gone, not migrated

`perry-ext-fetch` defined **74** `js_fetch_*` / `js_headers_*` / `js_response_*`
/ `js_request_*` / `js_blob_*` / `js_form_data_*` symbols. Every one of them is
also defined by `perry-stdlib`, which defines **24 more** — four of which
(`js_blob_new`, `js_file_new`, `js_headers_init_from_value`,
`js_fetch_notify_signal_aborted`) the *runtime* calls under
`external-fetch-symbols`, so a node-fetch build already needed the stdlib copy
in the link. The wrapper was a strict subset that shipped in front of the thing
it was a subset of.

And the two disagreed about what a handle is:

| | encoding | id space |
|---|---|---|
| `perry-stdlib/src/fetch/mod.rs` | `handle_to_f64(id) = js_nanbox_pointer(id)` — NaN-boxed POINTER_TAG | one counter in the fetch band `0x40000..0xE0000` |
| `perry-ext-fetch/src/lib.rs` | `JsValue::from_number(id as f64)` — a bare double | **six** per-type counters, each from **1** |

So Response #1, Headers #1 and Request #1 were all the double `1.0`, and a value
minted by one archive and read by the other is a small integer dereferenced as
an object pointer — P8's `segfault at 5`.

The routing made both archives present. The well-known flip maps `node-fetch` to
`["http-client"]` and strips it, but the stdlib symbols are gated on
**`web-fetch`**, a different feature, which `compute_required_features` inserts
independently whenever `uses_fetch` is true. Whichever archive won the link
decided whether the program crashed — and link order is not even stable: when
the strip/dedup transform falls back non-fatally,
`build_and_run.rs` reverts to stdlib-first with a warning.

**The fix is a deletion.** `node-fetch` and the bare `fetch` alias have no row in
`crates/perry/well_known_bindings.toml` any more, and the crate is gone.
`module_to_features` still maps `node-fetch` to `http-client`, which implies
`web-fetch` in cargo — and with no binding row, nothing strips it, so the stdlib
copy is *required* rather than optional.

What `node-fetch` gains, beyond not crashing: `AbortSignal` (perry#10325 — the
wrapper stored `signal` as a field and never consulted it: no `Notify`, no
`select!`, no `AbortError`), `Content-Encoding` decoding, connection pooling,
and P6's turnloop transport. None of those existed in the wrapper.

### The evidence the brief asked for: `nm` on the linked binary

On the **base** commit, the same `g1.ts` compiles against an auto-optimize
directory holding **two** definitions of the whole family:

```
$ nm --defined-only .../perry-auto-f2a918410ade618f/release/libperry_ext_fetch.a | grep -c ' T js_fetch_get$'
1
$ nm --defined-only .../perry-auto-f2a918410ade618f/release/libperry_stdlib.a    | grep -c ' T js_fetch_get$'
1
```

and the program that never calls the global `fetch()` gets a *single* definition
— the wrapper's — because `uses_fetch` is false and the stdlib copy is stripped:

```
$ nm --defined-only .../perry-auto-d3ef5d756493faf9/release/libperry_stdlib.a | grep -c ' T js_fetch_get$'
0
```

That is the whole bug in three commands: which copy you get depends on whether
some *other* line of the program calls `fetch`, and the two copies disagree.

On **P11**, `nm` on the linked binary says which archive won, and it is not a
count — it is an address. Both binaries were built with `PERRY_KEEP_SYMBOLS=1`
from the same `g1.ts`.

```
base 1edb5b7e8d                            P11
  js_fetch_with_options        0x10a5f0      0x945bdf
  js_fetch_response_status     0x109f00      0x945839
  js_fetch_handle_kind         0x9ee3eb      0x945065
  js_fetch_notify_signal_aborted 0x9ee6ab    0x945325
  js_headers_init_from_value   0x9f1e3b      0x94c06d

$ nm base_g1sym  | grep -c perry_ext_fetch      ->  112
$ nm perry_g1sym | grep -c perry_ext_fetch      ->    0
```

Read the base column. `js_fetch_with_options` and `js_fetch_response_status`
sit at `0x10…`, **immediately after `perry-ext-fetch`'s own mangled Rust
symbols**; `js_fetch_handle_kind`, `js_fetch_notify_signal_aborted` and
`js_headers_init_from_value` sit nine megabytes away at `0x9e…`, among
perry-stdlib's. Those last three are among the 24 symbols the wrapper never
shipped, so they *had* to come from the stdlib copy.

**The base binary runs two fetch implementations at once.** The request and
response family comes from the wrapper (bare-double handles, six counters from
1); the abort, handle-kind and headers-init family comes from perry-stdlib
(NaN-boxed, one fetch band). They are in the same process, handing each other
handles, and neither can read the other's. That is the SIGSEGV, in addresses.

On P11 every one of them is in one contiguous region — `0x945065`…`0x94c06d`,
perry-stdlib's — and **not a single `perry_ext_fetch` symbol is in the binary**,
because the archive does not exist in the tree:

```
$ ls target/perry-auto-*/release/libperry_ext_fetch.a
ls: cannot access '...': No such file or directory
$ nm --defined-only target/perry-auto-*/release/libperry_stdlib.a | grep -c ' T js_fetch_get$'
1
```

| | base `1edb5b7e8d` | P11 |
|---|---|---|
| `import 'node-fetch'` + global `fetch()` | **SIGSEGV (rc 139), 3/3** | `A/B/C: status = 200/D: len = 24`, rc 0, **3/3** |



## The `perry` CLI

Thirteen `reqwest::Client` constructions, seven `tokio::runtime::Runtime::new`
sites and two `tokio_tungstenite::connect_async` clients, replaced by
`perry-http-client`. Every `async fn` that existed only to be driven by one of
those runtimes is now a plain `fn`: `publish::run_async`,
`publish::preflight::run_security_audit_step`, `audit::run_audit_check`,
`verify::run_verify_check`, `login`'s device flow,
`run::remote::remote_build_and_launch`, `run::resign::resign_for_development`
and `run::resign::create_dev_profile_via_api`, and
`setup::ios::create_dev_profile_via_api`. Argument lists are unchanged
throughout; only `async` is gone. The three `tokio::time::sleep` calls (the
OAuth poll, the verify poll, the WebSocket reconnect backoff) are
`std::thread::sleep`.

Nothing in the CLI needed a work-stealing runtime. Every one of those call sites
was a `block_on` around a single request, or a poll loop that slept between
requests. A compiler driver has no JS event loop to cooperate with, which is why
the constraint P5, P6 and P7 worked under does not apply — see the next section.

### Where each site went

| site | was | now |
|---|---|---|
| `telemetry.rs:263` | blocking client, 3 s connect + 5 s request | `Client::with_timeout(8 s)` |
| `compat_reports.rs:453` | blocking client, 3 s + 5 s | `Client::with_timeout(8 s)` |
| `update_checker.rs:347` | blocking, 5 s + 10 s, UA `perry/<v>` | `Client::with_timeout(15 s).user_agent(...)` |
| `update_checker.rs:803` | blocking, 5 s + 300 s, UA | `Client::with_timeout(305 s).user_agent(...)`, and the artifact download now **streams** through a `BodySink` |
| `login.rs:79` | async, **no timeout** | `Client::new()` (120 s budget) |
| `audit.rs:204` | async, 120 s, `reqwest::multipart` | `Client::with_timeout(120 s)`, `perry_http_client::Form` |
| `verify.rs:130` | async, `timeout+30`, `reqwest::multipart` | `Client::with_timeout(timeout+30)`, `Form` |
| `run/remote.rs:162` | async, **no timeout**, multipart + WS | `Client::with_timeout(900 s)`, `Form`, `WebSocket` |
| `run/resign.rs:350` | async, **no timeout**, 8 ASC calls | `Client::new()` |
| `setup/ios.rs:91` | blocking, **no timeout**, 10 ASC calls | `Client::new()` |
| `setup/macos.rs:132` | blocking, **no timeout**, 3 ASC calls | `Client::new()` |
| `publish/mod.rs:1476` | async, **no timeout**, multipart + WS + reconnect | `Client::with_timeout(900 s)`, `Form`, `WebSocket` |
| `publish/server_api.rs` (`auto_register_license`) | async, **no timeout** | `Client::new()` |

### Behaviour changes, named

These are not transport-neutral, and a reviewer should see them as a list rather
than find them.

1. **Six call sites gained a timeout that had none.** `login`, `run --remote`,
   `publish`, `run/resign`, `setup ios` and `setup macos` all used
   `reqwest::Client::new()`, which has no deadline at all. They now inherit a
   whole-request budget (120 s, or 900 s for the two upload paths). A slow but
   working App Store Connect call that used to hang forever now fails.
2. **Timeout *shape* changed everywhere.** `reqwest`'s `.timeout()` bounded the
   response read and `.connect_timeout()` bounded the connect separately.
   `perry_http_client` measures one budget from the first connect attempt,
   shared across a redirect chain. The three sites that had both numbers were
   given their **sum**, so nothing got stricter there; the sites that had only
   `.timeout()` are now marginally stricter because the connect is inside the
   window.
3. **A 32 MiB ceiling on a buffered response body**, which `reqwest` did not
   have. Raised explicitly to 2 GiB at the two artifact downloads
   (`publish`, `run --remote`) rather than raised in the default, so the new
   refusal stays visible. The self-update artifact does not buffer at all.
4. **`User-Agent: perry/<version>` is now sent on every request.** `reqwest` sent
   it only from `update_checker`'s two clients; the other eleven sent none. P6
   recorded the other side of this — `api.github.com` answers 403 to an
   anonymous request — so this direction is the safe one, but it is wire-visible.
5. **`Accept: */*` and `Accept-Encoding: gzip, deflate` are now sent**, and the
   response is decompressed. reqwest was built with no decompression feature
   anywhere in this workspace, so it sent neither and would have handed a
   compressed body to `.text()` if a server had volunteered one.
6. **Two user-visible error strings lose the status reason phrase.**
   `reqwest::StatusCode`'s `Display` is `404 Not Found`; `Response::status` is a
   `u16`, so `verify.rs`, `audit.rs` and `resign.rs:607` now print `HTTP 404`
   where they printed `HTTP 404 Not Found`. `axios`'s `statusText` is NOT
   affected — see below.
7. **HTTP/2 is gone from the CLI's outbound requests.** `perry-http-client`
   advertises only `http/1.1` in ALPN and verifies the negotiated protocol, the
   same decision P6 took for `fetch`. A whole-workspace `cargo build` used to
   unify reqwest's features and give the CLI an h2-capable client; a
   `cargo build -p perry` did not. That inconsistency is also gone.
8. **`axios`'s `response.data` is no longer charset-transcoded.** `reqwest`'s
   `.text()` read the `charset` parameter and decoded through `encoding_rs`;
   this client's is `String::from_utf8_lossy`. Observable only for a non-UTF-8
   response, which for axios's JSON-shaped surface is rare — but real.
9. **`run --remote`'s WebSocket has a 900 s idle bound and no reconnect**, where
   it previously waited forever. `publish` got 600 s and takes its existing
   reconnect path.
10. **The publish WebSocket's "the stream ended" now has a deadline.** The async
   stream ended when the hub dropped the connection; a blocking read has to
   decide how long "nothing arrived" is. It is 600 s, and reaching it takes the
   same `reconnect_or_bail!` path a dropped stream took. The retry count,
   backoff and 60-retry cap are unchanged.

### `perry-updater` has nothing to migrate

The brief named it, so: `crates/perry-updater/Cargo.toml` depends on `anyhow`,
`perry-runtime`, `serde`, `semver`, `sha2`, `hex`, `base64` and `ed25519-dalek`.
No `tokio`, no `reqwest`, no transport crate at all, and `grep -rE
'reqwest|tokio|ureq|hyper'` over the crate returns exactly one hit — a test
fixture URL string. Its own module docs say why: *"Download lives in TS (using
existing `fetch()`) — Rust only handles the security-critical and
platform-touching pieces."* It verifies bytes that are already on disk. P8's
inventory agrees — it has no edge and never appears in the 46.

The bytes it verifies are fetched by `update_checker.rs`, which **is** in scope
and did move; that is the streaming download above.

### The count, against the brief's

The brief said 14 `reqwest::Client` constructions. It is **13 constructions**
plus one `&reqwest::blocking::Client` in a function signature
(`setup/macos.rs:452`, `create_apple_certificate`), which is where the
fourteenth mention comes from. 7 `Runtime::new` and 2 WebSocket clients are
exact.

## `perry-http-client`: why an owned loop is right here and nowhere else

P5, P6 and P7 all refused `turnloop_http::asynchronous` for the same two
reasons, and both are about sharing a thread with a JS event loop:
`LocalExecutor::with_config` builds its **own** `Driver`, and even sharing one,
`LocalExecutor::turn` drains completions into `Shared::dispatch`, which returns
early for any token without its own tag bit — so P1's net tokens, P2's process
tokens, P3's timer token and P4's pool tokens would be silently dropped
(PerryTS/turnloop#45).

Neither applies to a caller that owns no loop. Two qualify:

* **the `perry` CLI**, which is a compiler driver and has no JS agent at all;
* **a `perry-ext-*` binding's request body**, which already runs inside
  `perry_ffi::spawn_blocking` — a **tokio blocking-pool** thread that is not an
  agent, never parks on `js_*`, and used to block on
  `tokio::runtime::Handle::current().block_on` in exactly the same place.

So `perry-http-client` creates a `turnloop::Loop`, turns it to completion, and
drops it. That is the opposite shape from every other turnloop client in the
tree, and it is why this one is ~2,400 lines rather than a phase: no completion
sink, no promise bridge, no GC exposure, no keep-alive accounting, no
`aux_has_active` contributor. The crate documentation says who may use it and
nothing in `perry-runtime` or `perry-stdlib` depends on it.

What it is **not**: no connection pool, no HTTP/2, no cookie jar, no automatic
retry. Each is absent because no caller needs it, and adding one would be
adding an untested mode.

### `perry-tls-session`, extracted rather than copied

P6's outbound TLS client session lived in `perry-stdlib`. P6's own report named
the second copy (`perry-ext-net`'s server-side `turnloop_tls.rs`) as a
consolidation that never happened; the CLI would have made a third. It is now
`crates/perry-tls-session` — no socket, no loop, no I/O — driven from a
completion sink by perry-stdlib's `fetch` and SMTP engines and from a blocking
call by `perry-http-client`. `perry-stdlib/src/turnloop_tls_client.rs` is now
the `perry_ffi`-shaped `client_config()` and a re-export, 60 lines.

### multipart: the turnloop gap P8 named

P8 recorded that `turnloop_http::client` has no multipart form builder, and that
this blocked the CLI's uploads. `crates/perry-http-client/src/multipart.rs` is
the Perry-side answer: text fields and named file parts, in memory, which is
exactly what `perry publish`, `perry audit`, `perry verify` and
`perry run --remote` send (all four used `reqwest::multipart::Part::text` only —
`Part::stream` appears nowhere in the tree).

The part worth reviewing is the boundary. RFC 2046 lets one be arbitrary, and
most builders trust entropy. This one **scans every part's bytes and regenerates
until the boundary does not occur in any of them**, because three of the four
callers send base64 and a colliding boundary truncates an upload silently.
`the_chosen_boundary_never_occurs_inside_a_part` plants a boundary-shaped string
as a payload and asserts the chosen delimiter still appears exactly twice.

This still wants to be in `turnloop-http`, next to the chunked encoder — see
"turnloop gaps found".

## `axios`

`perry-ext-axios` keeps all eleven of its symbols, its handle encoding
(POINTER_TAG-tagged, per #340) and its perry-ffi handle registry. Only the
transport changed: `reqwest::Client::new()` + `Handle::current().block_on` inside
`spawn_blocking` became `perry_http_client::Client` on the same blocking-pool
thread.

That closes **perry#10326** as a side effect. The old code built a fresh
`reqwest::Client` — roughly 250 KB of state with its own cold DNS cache, TLS
session cache and connection pool — on **every call**. `perry_http_client` holds
no connection at all, so there is nothing per-call to throw away.

One thing had to be rebuilt rather than swapped: `statusText`. `reqwest` gave it
from `StatusCode::canonical_reason()`, and axios callers compare it against
literals, so it is observable. `reason_phrase()` is the table, covering every
code a real server sends; an unknown code answers `""`, which is what
`canonical_reason()` did.

What `perry-ext-axios` still does **not** have, unchanged by this lane: request
headers from JS, `axios.create()`, interceptors, `axios.request(config)`,
`response.headers`, auth, proxies, streaming, form-data, and `AbortSignal`. The
lowering is entirely codegen-driven static dispatch on the literal
`axios.<method>` shape.

`crates/perry-stdlib/src/axios.rs` is untouched and still on reqwest. It is
reachable only under `PERRY_DISABLE_WELL_KNOWN=1`, which is the reason P6 gave
for not migrating it and which still holds.

### axios, both arms, same fixture origin

Eleven assertions over every method the binding implements — `get`, `post`,
`put`, `patch`, `delete`, `head`, `options`, a 404, a JSON body round-tripped
through the echo, a raw-string body, and a `text/plain` response — against the
same local origin, three runs each:

```
get status = 200 statusText = OK
get data.name = p11
post status = 200
post echoed = {"a":1,"b":"two"}
put status = 200 echoed = {"u":true}
patch status = 200 echoed = raw-string-body
delete status = 200
head status = 200
options status = 200
404 status = 404 statusText = Not Found
text data typeof = undefined value = plain-text-body
rc=0
```

**Byte-identical on both arms** (`diff` over the two runs is empty), including
`statusText = Not Found` on the 404 — which is the `reason_phrase()` table
matching `canonical_reason()` — and including the pre-existing oddity on the
last line, which this lane did not introduce and did not fix.



## GC

Nothing in this lane holds a JS value across a thread or a completion that did
not already.

* **`perry-http-client` holds no JS value and no heap pointer.** It is `String`s,
  `Vec<u8>`s and a `turnloop::Loop`. It has no `perry-ffi` dependency and cannot
  see a `JSValue`.
* **`perry-tls-session` is P6's code moved, not changed** — owned ciphertext and
  plaintext buffers, no root, no scanner. P6's GC reasoning transfers verbatim.
* **`perry-ext-axios`'s exposure is unchanged.** The same `spawn_blocking`
  closure, the same `JsPromise` pinned across the crossing, the same
  `register_handle`. What it holds while blocked is now a `Loop` instead of a
  `reqwest::Client`; neither is a GC root.
* **`perry-ext-fetch`'s GC root scanner is deleted with the crate.** It
  registered `scan_fetch_roots` for `RequestData::signal`; `perry-stdlib`'s fetch
  has its own handling of the same thing and is now the only implementation.
  `scripts/gc_runtime_root_holders.json` loses that entry in the same commit, as
  the gate requires — it fails on a stale entry.
* **No new `gc_register_mutable_root_scanner` call**, because there is no new
  cache of a heap pointer.

`scripts/gc_runtime_root_holders.py` is green on this branch.

**No GC-stress arm was run, and that is a decision rather than an omission.**
The knobs stress a collector against code that holds GC values across collection
points. The CLI has no collector — it is a Rust binary with no JS heap. The two
bindings' JS-value exposure is byte-identical to what it was before this lane
(same promise, same registry, same closure); the only thing that changed inside
the `spawn_blocking` body is which socket library it blocks on, and that body
already could not touch a JS value. Running `PERRY_GC_SCHEDULE_SEED` over it
would produce a green with no subject, which is exactly the shape CLAUDE.md's
"★ Four ways a gate can be unable to fail" warns about.

## Test evidence

Every command as run, on the shared Linux box, against Node **26.5.1**. Both
arms were built from source in their own clone on that box, from the same
package set, and the baseline is this branch's **own base commit** —
`1edb5b7e8d` in `/root/claude-turnloop-p11/base`, not the committed snapshot and
not another session's binary.

```
cargo build --release --locked \
  -p perry -p perry-runtime -p perry-stdlib -p perry-runtime-static -p perry-stdlib-static \
  -p perry-ext-http -p perry-ext-net -p perry-ext-ws -p perry-ext-zlib -p perry-ext-events \
  -p perry-ext-axios
```

**The package sets are identical except that `perry-ext-fetch` cannot be in the
P11 set, because the crate does not exist there.** The base tree was rebuilt
with this exact set after its first build, so the difference is only the crate
this lane deletes.

**The swept P11 binaries are at `24bed7439b`, and HEAD is two commits later.**
That is not the "docs only" case P8 could claim, so here is what changed and why
no gap test can see it:

```
$ git diff --stat 24bed7439b..HEAD -- crates/ Cargo.toml Cargo.lock
 crates/perry-http-client/examples/probe.rs    |  88 ++++++++-
 crates/perry-http-client/examples/ws_probe.rs |  60 +++++++
 crates/perry-http-client/src/http.rs          | 148 +++++++++-----

$ cargo tree -i perry-http-client --workspace -e normal
perry-http-client
├── perry              (the CLI binary — not linked into any compiled program)
└── perry-ext-axios    (linked only by a program that imports 'axios')

$ grep -l "from ['\"]axios" test-files/test_gap_*.ts | wc -l
0
```

Two of the three files are `examples/`, which nothing links. The third is
`perry-http-client`'s request driver, whose only two reverse dependencies are
the CLI binary — which a gap run invokes for `compile`, never for a network
subcommand — and `perry-ext-axios`, which **no `test_gap_*` fixture imports**.
So the changed code is not in any swept fixture's link, and the `perry` binary's
copy of it is on a path a gap run does not execute. The change is the
double-fetch fix described under "The streamed download".

### Unit tests

```
cargo test -p perry-http-client   ->  42 passed, 0 failed  (+1 doc-test)
```

Four of them exist because something was wrong and the test is what keeps it
that way: `the_end_event_arrives_from_a_step_that_consumes_nothing` (see below),
`a_streamed_request_issues_one_hop_per_redirect_and_no_more`,
`the_redirect_ceiling_matches_the_one_reqwest_used` and
`a_content_encoding_list_is_split_innermost_last`. Two of them are written so
they fail if they stop discriminating rather than going quiet — the `End` test
asserts the *old* rule still fails, and the redirect test asserts turnloop's
constant is still different from 10.

### `perry-http-client` against real servers

Not a gap fixture, because it needs the network. This is what establishes that
the transport, the TLS session and the HTTP/1 codec actually talk to a server:

```
$ target/release/examples/probe https://example.com/ https://api.github.com/meta \
    http://github.com/ https://crates.io/api/v1/crates/serde \
    https://registry.npmjs.org/-/package/typescript/dist-tags \
    https://hub.perryts.com/api/v1/version/latest
OK   https://example.com/ -> 200 559 bytes  final=https://example.com/  ct=text/html
OK   https://api.github.com/meta -> 200 153006 bytes  ct=application/json; charset=utf-8
OK   http://github.com/ -> 200 577307 bytes  final=https://github.com/  ct=text/html; charset=utf-8
OK   https://crates.io/api/v1/crates/serde -> 200 440989 bytes  ct=application/json
OK   https://registry.npmjs.org/-/package/typescript/dist-tags -> 200 179 bytes
OK   https://hub.perryts.com/api/v1/version/latest -> 404 21 bytes  ct=application/json
rc=0
```

Four things that a loopback fixture cannot establish are in that output: a real
certificate chain verified against the webpki roots; a cross-scheme
`http://github.com/` → `https://github.com/` redirect, visible in `final=`; a
response HEAD that spans several reads (github.com, the exact case P6's report
says every loopback fixture passes through); and `api.github.com` answering 200
rather than 403, which is the default `User-Agent` working.

### The bug the previous session left behind, and the test that pins it

This lane inherited a partly-written `perry-http-client` from an interrupted
session. It had never been run. Its first real-server probe failed on **every**
URL, including plain `http://github.com/`:

```
FAIL https://example.com/ -> response timed out
FAIL http://github.com/ -> response timed out
```

but it passed against a local `Connection: close` fixture. The cause is exactly
P6's five-second bug, in a place where nothing rescues it: `http1::Decoder`
emits `Event::End` from a step that consumes **zero** bytes, and the feed loop
was written as `while offset < pending.len()`. A keep-alive response hands over
every byte and then never completes, so the request sits until the whole-request
deadline. P6's engine survived it at five seconds per request because the
server's own idle timeout finished the exchange; here there is no pool and no
keep-alive rescue, so it is 30 seconds and a hard failure.

`the_end_event_arrives_from_a_step_that_consumes_nothing` drives one real
keep-alive response through **both** loop rules and asserts the old one does not
reach `End` while the new one does. Its first assertion is what keeps it honest:
if `turnloop-http` ever starts consuming a byte for `End`, the old rule would
pass too and the test would stop discriminating — so it fails rather than going
quiet.

The lesson is the one P6 already wrote down and this lane paid for again: **a
loopback fixture is not evidence for an HTTP client.**

### Every CLI command that did network I/O, run on both arms

A stand-in Perry Hub / verify service (`hub.py` in the lane's scratch tree)
answers the real endpoint shapes and **parses the multipart bodies itself**, so
a form that lost a part shows up as a missing field name rather than as a
passing test. Both arms ran the identical script against the identical server,
from a cleared `~/.perry` so nothing short-circuited on a cached token.

| command | base `1edb5b7e8d` | P11 |
|---|---|---|
| `perry login --server <hub>` | `✓ Logged in as @octocat (pro)`, rc=0 | identical |
| `perry audit <dir> --verify-url <hub>` | `✓ Audit grade: A (0 findings)`, rc=0 | identical |
| `perry verify fake.bin --verify-url <hub>` | `→ Job submitted: v-1` / `✓ Verification passed`, rc=0 | identical |
| `perry update --check-only` (the REAL release ladder) | `Perry is up to date (v0.5.1580)`, rc=0 | identical |
| `perry publish linux --server <hub>` | packaged, uploaded, WS progress, `✓ Build completed in 1.0s`, `Downloading app.bin... done → dist/app.bin`, rc=0 | identical |

`diff` over the two arms' complete stdout — normalising only the tree path, the
random OAuth device code and the random `Sec-WebSocket-Key` — is **empty**.

And what the hub actually received — the multipart fields and their byte
lengths, which is where a hand-written RFC 7578 builder would differ from
reqwest's if it differed at all:

```
POST /api/cli/start  ctype='application/json' body={"device_code":"..."}
GET  /api/cli/poll?code=...
POST /audit          multipart parts=[('source', 53), ('config', 66)]
POST /verify         multipart parts=[('binary_b64', 24), ('target', 9), ('config', 28), ('manifest', 61)]
GET  /verify/v-1
POST /api/v1/build   multipart parts=[('manifest', 254), ('credentials', 115), ('tarball_b64', 540)]
     authorization=Bearer tok-abc
WS   upgrade version=13
WS   first frame opcode=1 payload=b'{"type":"subscribe","job_id":"job-1"}'
GET  /artifact/app.bin -> 4111 bytes
```

That block is the **base** arm's. The P11 arm produced the identical lines —
same field names, same byte lengths, same bearer header, same subscribe frame,
same artifact fetch. A hand-written RFC 7578 builder that differed from
reqwest's by a byte would show up as a different length here.

**Not exercised end to end: `perry setup ios`, `perry setup macos`, and the App
Store Connect half of `perry run --target ios`.** Those talk to
`https://api.appstoreconnect.apple.com` at a hard-coded host with an ES256 JWT
signed by an Apple private key, and there is no way to point them at a local
server. Their 21 request sites were migrated mechanically (`.bearer_auth` →
`.bearer`, `.query(&[..])` → `.query(&[..])`, `resp.json()` →
`serde_json::from_slice(&resp.body)`) and they compile, and that is the whole of
the evidence for them. Say so rather than let the table above imply otherwise.

### `node-fetch`, the two shapes, and the defect that is still open

Two probes, differing only in what the default import is **named**:

```ts
import fetch     from "node-fetch";   // nf2.ts  — what a real program writes
import nodeFetch from "node-fetch";   // nf_only.ts
```

| | base `1edb5b7e8d` | P11 |
|---|---|---|
| `nf2.ts` — bound to `fetch` | **SIGSEGV, 3/3** | `typeof r = object` / `status = 200` / `len = 24`, rc 0, **3/3** |
| `nf_only.ts` — bound to `nodeFetch` | `TypeError: … reading 'status'`, 3/3 | **`TypeError: … reading 'status'`, 3/3 — unchanged** |

The first row is the fix. The second row is a **different defect**, and this
lane's change is what separated them — P8 saw the `undefined` and wrote that it
"may be the bare-number handle above, or it may be that the awaited value's type
is not proven to be a `Response` at the property site", and did not chase it.
Both hypotheses were right, for different programs:

* bound to **`fetch`**, HIR sets `uses_fetch`, `web-fetch` is required, **both**
  archives were linked, and the handle a bare double is read as a pointer →
  SIGSEGV. Removing the wrapper removes this entirely.
* bound to **anything else**, `uses_fetch` stays false. On the base commit only
  the wrapper was linked and its bare-double handle produced `undefined`. On
  P11 the *symbols are right* — `nm` puts `js_fetch_with_options` at `0x944c71`
  next to `js_fetch_handle_kind` at `0x9442c6`, i.e. perry-stdlib's — but
  **`js_fetch_response_status` is not in the binary at all**. Codegen lowers the
  *call* and not the *property read*, so `r.status` is a generic property lookup
  on a handle, which is `undefined`.

That second one is a codegen/HIR defect about alias tracking, not a transport
one, it is pre-existing on both arms, and it wants its own issue. It is named in
"Perry defects this work found".

Note the Node oracle cannot run either probe: `node-fetch` is not in the
repository's `package.json`, which is the same reason P6 gave for its SMTP
fixture. The comparison above is arm-against-arm.

### The streamed download, and the bug it caught

`perform_self_update` is the only streaming transfer in the tree and the only
`BodySink` implementation, so `probe --stream <url> <path>` exercises the same
shape: a real GitHub **release asset**, which is the exact case the self-updater
hits — a `github.com/.../releases/download/...` URL that 302s to
`release-assets.githubusercontent.com`.

```
$ probe --stream https://github.com/PerryTS/perry/releases/download/v0.5.1520/\
    perry-cross-aarch64-apple-darwin.tar.gz  /tmp/p11_stream.bin
     head #1 status=200 content-length=Some(47533003)
OK   ... -> 200 final=https://release-assets.githubusercontent.com/...
     heads=1 declared=Some(47533003) written=47533003 buffered=0
rc=0
-rw-r--r-- 1 root root 47533003 /tmp/p11_stream.bin
```

`heads=1` is the load-bearing number: the head was offered to the sink **once**,
on the final response, not once per redirect hop. `written == declared` says
every byte arrived. `buffered=0` says nothing was held in memory.

And the same probe against a release asset that does not exist, which is the
ordering the review caught:

```
OK   .../v0.0.0-nope/missing.tar.gz -> 404
     heads=0 declared=None written=0 buffered=9
-rw-r--r-- 1 root root 0 /tmp/p11_stream404.bin
```

`written=0` and a **zero-byte file**: the 404 body never reached the sink, so
`error_for_status()` sees it — with the error text, in `buffered=9` — before
anything is on disk. That is `reqwest::send()`'s ordering, which returned on the
head; an earlier draft here wrote the error page into the self-updater's staging
file first and only then reported the status.

Both re-run against the final code, with `turnloop`, `turnloop-http` and
`turnloop-tls` pinned to the exact `=0.1.0-alpha.3` the workspace uses.

**This probe caught a real bug in this lane's own code, and the file above is
the case that would have failed.** The first version of `execute_streaming`
followed the redirect chain with the sink *withheld* — buffering each hop to read
its status — and then **re-issued the final hop** with the sink attached. That
fetched the artifact twice, and the discarded first copy was measured against
`max_body`, whose default is 32 MiB. A 45 MB release asset would have been
refused with "response body exceeds 33554432 bytes" before a byte reached disk.

The fix is to let the sink ride along on every hop and decide *per response*,
from the status, whether its body may reach the sink: a 3xx is buffered and
discarded, anything else streams. `a_streamed_request_issues_one_hop_per_redirect_and_no_more`
pins it — it reads `run()`'s own source and fails if a second `one_hop` call
appears, which is the only shape the re-issue can take.

### The WebSocket client, end to end

`perry publish` and `perry run --remote` are the only WebSocket clients in the
tree, and this is the one genuinely new protocol implementation in the lane, so
its framing unit tests (13 of them, driven with bytes) are not enough on their
own. `cargo run -p perry-http-client --example ws_probe -- <url>`:

```
=== a local server that pushes six frames in one segment, then closes ===
connected ws://127.0.0.1:41100/ws/job-1
sent subscribe
text[1] {"type": "job_created", "job_id": "job-1", "position": 1}
text[2] {"type": "queue_update", "position": 1}
text[3] {"type": "stage", "stage": "compiling", "message": "compiling"}
text[4] {"type": "log", "stage": "compiling", "line": "tick", "stream": "stderr"}
text[5] {"type": "progress", "stage": "compiling", "percent": 50}
text[6] {"type": "artifact_ready", "artifact_name": "app.bin", ...}
peer closed after 6 message(s)
rc=0

=== wss://echo.websocket.org/ — a real TLS handshake and a real peer ===
connected wss://echo.websocket.org/
sent subscribe
text[1] Request served by 4d896d95b55478
text[2] {"type":"subscribe","job_id":"job-1"}
```

The second one is the masking proof: the echo service unmasked the frame and
sent the bytes back, so the RFC 6455 §5.3 masking key and the `Sec-WebSocket-Key`
/ `Sec-WebSocket-Accept` handshake are both right against something that is not
this lane's own code. The first proves several frames are drained out of one
read rather than one per read, and that the peer's close is `Ok(None)` and not
an error.

### The gap suite, against a baseline built from this branch's own base

**The first pass of both arms was killed by something outside this lane**, at
test 539 (base) and 533 (P11) of 818, with `GAP_base_RC=143` and
`GAP_perry_RC=143` — SIGTERM, simultaneously, with no OOM in `dmesg` and 139 GB
of memory free. The build box runs four or five lanes at once and CLAUDE.md's
brief warns that an unanchored `pkill -f` has already destroyed other lanes'
multi-hour sweeps twice. The partial result is still worth reporting, because it
is a per-test comparison over two-thirds of the suite:

| | base `1edb5b7e8d` | P11 (`24bed7439b`) |
|---|---|---|
| tests reached before the kill | 528 | 521 |
| pass | 524 | 517 |
| parity_fail | **4** | **4 — the same four** |
| compile_fail / crash | 0 | 0 |
| **status changes on the 521 common tests** | — | **0** |

The four are `2159_defineproperty_class_prototype`, `2514_settracesigint`,
`2899_2779_2777_static_helpers` and `disposablestack_2875` — all four in P6's
and P8's lists, none of them this lane's, and two of them
(`…_static_helpers`, `disposablestack_2875`) among the three the committed
snapshot expects to PASS and which are red on the base commit before this branch
changes anything.

#### The complete sweep, both arms

Both arms were then re-run from scratch, and the P11 arm was **rebuilt at the
branch's own HEAD first** so the swept binary is this branch's code rather than
the commit the interrupted pass had used:

| | base `1edb5b7e8d` | **P11 `8773f388bb`** |
|---|---|---|
| tests run | 818 | 818 |
| pass | 809 | **809** |
| parity_fail | **9** | **9 — the same nine** |
| compile_fail | 0 | **0** |
| crash | 0 | **0** |
| skipped | 0 | 0 |
| parity rate | 98.8 % | **98.8 %** |
| harness exit | 1 | 1 |
| **status changes, compared per test** | — | **0** |

Compared from the two JSONL journals test by test, not from the totals — the
two runs share all 818 test ids and **not one of them differs**:

```
$ compare_gap.py base-final.jsonl p11-final.jsonl
A: 818 tests  head=1edb5b7e8d  bin=.../base/target/release/perry
   {'parity_fail': 9, 'pass': 809}
B: 818 tests  head=8773f388bb  bin=.../perry/target/release/perry
   {'parity_fail': 9, 'pass': 809}
COMMON TESTS: 818
STATUS CHANGES: 0
```

The nine, byte-identical sets on both arms and none of them this lane's:

```
2159_defineproperty_class_prototype   json_lazy_defineproperty_index
2514_settracesigint                   perfhooks_3088_3008_3010_3011
2899_2779_2777_static_helpers         prop_plan_cache_invalidation
disposablestack_2875                  v8_2_3680plus
iterator_prototype_next_patch
```

Both arms exit 1 for the same reason and print the same three lines: three of
those nine (`…_static_helpers`, `disposablestack_2875`,
`iterator_prototype_next_patch`) are expected to PASS by the committed snapshot
and are red on the base commit before this branch changes anything. That is
exactly why the comparison here is arm-against-arm rather than against the
snapshot.

Two fixtures are worth naming individually because they are the ones this lane
could have broken: **`test_gap_turnloop_fetch`** — P6's thirteen-case fetch
fixture — and **`test_gap_fetch_reqresp_2640_2643`** both PASS on the arm where
`perry-ext-fetch` does not exist, which is the sweep's own statement that
routing `node-fetch` to perry-stdlib did not disturb the global `fetch`.

**`compile_fail 0` on both arms is load-bearing** for a different reason (#7629):
the gap suite links whatever `perry-ext-*` archive is already in the tree, and
an incoherent one makes every `http`/`net` fixture fail to *compile* with "the
wrapper archive bundles a DIFFERENT tokio compilation than the stdlib archive" —
indistinguishable from a real regression. Deleting a wrapper crate is precisely
the change that could have caused it.

### Local gates

Run from the branch on the macOS development host unless noted:

| gate | result |
|---|---|
| `cargo test -p perry-http-client` | **42 passed** (+1 doc-test) |
| `cargo check --workspace` (less the cross-host UI crates) | clean |
| `cargo fmt --all -- --check` | OK |
| `./scripts/check_file_size.sh` | OK |
| `python3 scripts/addr_class_inventory.py` | OK |
| `python3 scripts/check_test_registration.py` | OK |
| `python3 scripts/tokio_inventory.py` | **OK — 39 edges, 13 crates** |
| `python3 scripts/gc_runtime_root_holders.py` | OK |
| `python3 scripts/binding_governance.py --check` | OK (the generated table in `docs/src/native-libraries/governance.md` is regenerated) |
| `python3 scripts/workspace_architecture.py --check` | **OK — and it was RED on the base commit**, see below |
| `python3 scripts/unrooted_local_shape.py --check` | **FAIL — red on the base commit too**, see below |
| `cargo clippy -p perry-http-client -p perry-tls-session -p perry-ext-axios` | no findings in those three crates |
| `cargo check -p perry-stdlib --no-default-features --features full` | clean (the pre-existing `redis v1.6.0` future-incompat note only) |
| `cargo check -p perry` warnings | **10, and all 10 are on the base commit too** — nine dead-code warnings in P2's `perry-runtime/src/turnloop_proc` plus the roll-up line. Identical string-for-string on both arms. |

Two of those want explaining, because both were already failing before this
branch and one of them changed *which* failure it reports.

**`workspace_architecture.py --check` was red on `1edb5b7e8d`** with
`unclassified workspace crates: perry-db-turnloop` — P7 added that crate and did
not classify it. This lane adds three entries (`perry-db-turnloop`,
`perry-tls-session`, `perry-http-client`, all `runtime-core` / `keep`) and
refreshes the baseline, because deleting `perry-ext-fetch` changes the member
count and the dependency closures anyway. The gate is green now; classifying
another lane's crate to get there is called out rather than buried.

**`unrooted_local_shape.py --check` is red on both arms, for the same
pre-existing cause, and this lane changed the message.** On the base commit it
fails at the first check — `REGRESSION: 567 findings exceeds baseline 561`,
which `return`s before the per-file loop runs. Deleting `perry-ext-fetch` takes
the total to 539, below the baseline, so the per-file loop now runs and reports
what the total was masking: `crates/perry-ext-pg/src/turnloop_io/result.rs: 4
findings exceeds per-file ceiling 0` — a P7 file that was never given a baseline
entry. **This lane deliberately did not re-baseline it.** Recording another
lane's debt under this lane's name is how a ratchet stops being one; it belongs
to whoever owns P7's change, and it is now visible instead of hidden behind a
total.

## What an independent review of this branch found

The diff was reviewed against the base commit by a reader with no stake in it,
and it found six things worth fixing plus two worth naming. All six are fixed on
this branch; the review is the reason they are, and the report says so rather
than presenting a clean diff that was clean on the second attempt.

1. **TLS: this crate was reading `NODE_TLS_REJECT_UNAUTHORIZED`,
   `NODE_EXTRA_CA_CERTS` and `SSL_CERT_FILE`, and the doc said reqwest had
   honoured them here.** It had not — the CLI used the workspace `reqwest` with
   `rustls-tls` and webpki roots and no environment handling at all, and old
   axios built a bare `reqwest::Client::new()`. Only the now-deleted
   `perry-ext-fetch` read them, through `perry_ffi`. So this was a *new*
   capability described as a preserved one, and its shape is the bad one: a
   variable JS developers set casually for an unrelated program would silently
   turn off certificate verification for `perry publish`, which uploads Apple
   signing certificates, API tokens and licence keys. **Removed.** Verification
   is always on and not configurable from this crate; a corporate-CA story for
   the CLI is a feature with its own decision and its own test.
2. **`Content-Encoding` is a list, and `turnloop_http::compression` matches one
   token exactly.** A legal `gzip, gzip` — or two `Content-Encoding` header
   lines — failed the whole response with `UND_ERR_NOT_SUPPORTED`. New failure
   mode, reachable the moment this client started sending `Accept-Encoding` of
   its own, which the reqwest build never did. **Fixed**: the list is split and
   applied innermost-last, across every header line.
3. **A non-2xx body was streamed to the sink.** `reqwest::send()` returned on
   the head, so `error_for_status()` always ran before a byte was copied; the
   new path wrote a 404 error page into the self-updater's staging file and
   then reported the error. Not exploitable — the staging dir is a tempdir and
   the manifest hash gates installation — but a real ordering change. **Fixed**:
   only a 2xx streams; everything else buffers and is returned.
4. **The whole-request budget could be exceeded by a factor of the resolved
   address count.** `Connection::connect` gave the full budget to *each*
   address, and `one_hop` then re-anchored the full budget again for the
   response. A host with four A and four AAAA records could spend eight times a
   120 s window before the first byte was sent. **Fixed**: one budget across
   every address, and the response gets what is left of it.
5. **The redirect ceiling had gone 10 → 20** by reading
   `turnloop_http::client::DEFAULT_MAX_REDIRECTS`; reqwest's default, which
   every CLI call site took, is 10. **Fixed**, with a test that fails if
   turnloop's constant ever becomes 10 and the test stops discriminating.
6. **`flush_tls_output` was the one unbounded loop** in `transport.rs`; the
   other four have a spin cap and a diagnostic. **Fixed.** Also: axios's
   `reason_phrase` was missing `103 Early Hints` and `425 Too Early`, and
   `perry-tls-session::node_message` was made `pub` by the extraction when
   nothing outside the crate calls it. Both fixed.

Two findings are recorded rather than fixed, and both are named in "Behaviour
changes":

* **`axios`'s `data` loses charset transcoding.** `reqwest::Response::text()`
  reads the `charset` parameter and transcodes through `encoding_rs`; this
  client's `text()` is `String::from_utf8_lossy`. A
  `text/html; charset=iso-8859-1` response now arrives with replacement
  characters. Fixing it means a new dependency for a surface whose `data` is
  almost always JSON, so it is a decision for whoever owns the binding.
* **`run --remote`'s WebSocket now has a 900 s idle bound and no reconnect**,
  where `publish` got 600 s *with* the existing reconnect loop and the old code
  waited forever. A hub that legitimately goes quiet for more than fifteen
  minutes mid-build now fails the command instead of hanging. Both are bad;
  which is worse is a product call.

The review also confirmed three things this report asserts, by reading the
code rather than trusting it: the feed loop cannot spin or drop bytes (the only
zero-consume events the decoder emits are `End` and `Upgrade`, and both
transition to `Done` in the same step); `perry-tls-session` is a pure move with
no logic line changed; and `update_checker`'s byte accounting, `require_https`
calls and `verify_cli_artifact` gate are all intact.

## turnloop gaps found

Reported here in the shape P5, P6 and P8's were; the coordinator files them.

1. **`turnloop_http::client` still has no multipart form builder** — P8's item,
   restated because this lane had to write one rather than route around it.
   `crates/perry-http-client/src/multipart.rs` is 280 lines and belongs next to
   `http1::Encoder`. The part that is not obvious and which a crate-side builder
   should keep: the boundary must be **verified absent from the parts**, not
   merely drawn from entropy, because a base64 payload can contain one and the
   failure is a silently truncated upload.
2. **`http1::Decoder` emits `Event::End` from a step that consumes zero bytes,
   and still nothing says so.** P6 reported this and it cost this lane a full
   debugging cycle on inherited code that had been written to the obvious
   contract ("loop while there is input"). The symptom is not a hang but a
   *timeout*, which reads as a network problem rather than a codec contract
   problem. A `Decoder::wants_step()` predicate, or one sentence in `Step`'s
   docs, would close it. This is now the second lane to pay for it.
3. **There is no way to ask `http1::Decoder` whether it is mid-message.** A
   blocking client has to decide, after a read returns zero, whether EOF is a
   legal end of body (`Connection: close`) or a truncated response. `eof()`
   answers by erroring or not, which means the only way to ask is to tell it.
4. **`turnloop_tls::ClientConfig` hardcodes `rustls::crypto::ring`** and
   **cannot express a client certificate** — P6's items 4 and 5, unchanged, and
   now with a second consumer. `perry-http-client` links `ring` for that reason
   alone, including into `libperry_ext_axios.a`.
5. **`turnloop::Loop` has no cheap "one socket" configuration.** `Config`'s
   defaults size tables for a server; a CLI that makes one request at a time
   trims `max_handles` to 8 and `max_operations` to 32 by hand. A
   `Config::single_connection()` (or documented guidance on what is safe to
   trim) would stop every such caller guessing.
6. **`TcpOpts` still exposes only `nodelay`** — P1's and P6's item, unchanged.
7. **`LocalExecutor` silently drops completions it did not issue**
   (PerryTS/turnloop#45) — P5's finding, still the reason the sans-I/O path is
   the only one Perry can use from a loop-owning thread. It is *not* why this
   lane is sans-I/O — a CLI could have used `LocalExecutor` — but the
   alternative would have put a second `Driver` inside `perry-ext-axios`, on a
   thread pool shared with the runtime, and that is not a bet worth taking for
   a saving of a few hundred lines.

## Perry defects this work found

Each reproduced on the base commit, in this lane's own clone, on the same box.

1. **`import 'node-fetch'` + a global `fetch()` SIGSEGVs** — P8's defect 1.
   **Fixed**, by deleting the duplicate rather than reconciling it. See the
   `nm` evidence above. P8 measured the same crash on `main`, so this is a
   `main`-line fix riding on a turnloop branch; an integrator who wants it
   sooner can cherry-pick the `well_known_bindings.toml` row removal and the
   crate deletion without any of the transport work.
2. **A node-fetch default import bound to a name other than `fetch` loses its
   response properties.** `import nodeFetch from 'node-fetch'; (await
   nodeFetch(u)).status` is `undefined` on the base commit **and on this
   branch**. Newly isolated: on P11 the symbols are perry-stdlib's and correct
   (`nm` puts `js_fetch_with_options` at `0x944c71`, next to
   `js_fetch_handle_kind`), but **`js_fetch_response_status` is not emitted at
   all** — codegen lowers the call and not the property read, so `.status` is a
   generic lookup on a handle. It is an alias-tracking defect in HIR/codegen,
   not a transport one, and it wants its own issue. Related to, and probably the
   same root as, `uses_fetch` not being set for the aliased binding.
3. **Both `axios` copies built a fresh `reqwest::Client` per request**
   (perry#10326) — **fixed for `perry-ext-axios`**, which is the copy
   `import 'axios'` links. `crates/perry-stdlib/src/axios.rs` still does it and
   is still only reachable under `PERRY_DISABLE_WELL_KNOWN=1`.
4. **`perry-ext-fetch` had no `AbortSignal` wiring at all** (perry#10325) — P6's
   defect 3. Fixed by deletion: `node-fetch` now reaches perry-stdlib's fetch,
   whose abort path P6 repaired.
5. **`scripts/unrooted_local_shape.py`'s two checks are ordered so the first
   masks the second.** `total > baseline` returns before the per-file loop, so a
   per-file regression is invisible while any total regression stands. That is
   how P7's `perry-ext-pg/src/turnloop_io/result.rs` sat unreported; this lane
   found it only by *lowering* the total. The two checks should both run and
   both report.
6. **`scripts/workspace_architecture.py --check` was red on
   `turnloop/integration`** before this branch — P8 found the same shape with
   `gc_runtime_root_holders.py` on its own base. Two of the integration
   branch's lint gates have now been left red by a merged lane; the pattern is
   worth a process note rather than another one-off fix.
7. **The CLI's reqwest features depended on what else was in the cargo
   invocation.** `crates/perry/Cargo.toml` took the workspace `reqwest`
   (`default-features = false`, no `http2`), but `perry-stdlib`,
   `perry-ext-fetch` and `perry-ext-http` each asked for `http2` — so
   `cargo build --release` (whole workspace) gave the `perry` binary an
   h2-capable client and `cargo build -p perry` did not. Same class as the
   `js_regexp_test` cfg-unification incident. Gone with the dependency.

## Liveness: proving the subject actually changed

The evidence standard asks for a counter that says the new code ran rather than
a green that says nothing broke. The CLI has no `PERRY_LOOP_STATS` — it is a
Rust binary with no JS agent — so the equivalents are what the binary contains
and how many threads it starts.

**Embedded crate source paths in `target/release/perry`** (`strings -a`), which
the panic machinery leaves behind for every crate actually compiled in:

| | base `1edb5b7e8d` | P11 |
|---|---|---|
| `tokio-1.` | **49** | **0** |
| `reqwest-0.12` | **14** | **0** |
| `tokio-tungstenite-` | **11** | **0** |
| `tungstenite-0.` | **19** | **0** |
| `hyper-1.` | **17** | **0** |
| `perry-http-client` | 0 | **5** |
| `rustls-0.23` | 33 | 33 |
| binary size | 104,909,856 | **103,115,808** (−1.79 MB) |

**`libperry_ext_axios.a`**, the archive `import 'axios'` links into a compiled
program, drops from **71,676,480 to 60,711,548 bytes** — 11 MB, 15 %. The
turnloop stack (`turnloop` + `turnloop-http` + `turnloop-tls` + rustls) is
smaller than reqwest + hyper + tokio, even though rustls is in both.

**Peak OS threads during one `perry verify`** (a multipart submit and a poll
loop), sampled from `/proc/<pid>/task` while the command ran, on the 64-thread
box:

| | base | P11 |
|---|---|---|
| peak threads | **66** | **2** |

That is the shape of the change in one number: the old CLI stood up a
work-stealing runtime with one worker per core to make two HTTP requests. The
new one is the main thread plus the background update-check thread. It is a
thread count, **not** a benchmark — nothing here was timed, and the box was
running four other lanes' work throughout.

For `perry-ext-axios` the liveness proof is different and is in the axios
section: eleven assertions across seven HTTP methods produce byte-identical
output on both arms, which cannot happen unless the new transport served every
one of them. For `node-fetch` it is the `nm` addresses.

## What was not run

Named precisely.

* **Windows and macOS.** Everything above ran on Linux x86_64. The CLI is the
  one crate in this lane that ships to both, and neither was exercised. The
  Windows path is the one to look at: `perry-http-client`'s only platform
  assumption is `ToSocketAddrs` and turnloop's own backend, but nothing proves
  it.
* **`perry setup ios`, `perry setup macos`, and `perry run --target ios`'s
  App Store Connect calls.** 21 request sites, migrated mechanically, compiled,
  never run — the host is hard-coded and the credential is an Apple private key.
* **`perry run --remote` end to end.** It shares `auto_register_license`, the
  multipart upload shape, the WebSocket and the artifact download with
  `perry publish`, which *was* run end to end on both arms; but the `run --remote`
  entry point itself was not driven.
* **`perform_self_update` end to end** — that means letting the CLI replace its
  own binary from a real signed release, and it was not done.
  `perry update --check-only` (the release-info ladder, the same client) ran on
  both arms and is identical, and the *streaming transport underneath it* is
  exercised directly — see "The streamed download" below. What is untested is
  the composition: the manifest verification, `ensure_complete_download`, the
  extraction and the transactional install, all of which this lane did not
  change.
* **A benchmark.** The box was at load 30–70 with four other lanes running gap
  sweeps throughout, and the brief forbids timing there. No number in this
  report is a performance claim.
* **`cargo test --workspace`**, and `perry-runtime`'s unit suite.
* **The auto-optimize gap tier.** Only the fast tier ran.
* **A build of the `tokio-wait-driver` A/B arm.** Nothing in this lane reads
  that feature — the CLI never did, and `perry-http-client` has no `cfg` on it —
  but the arm was not built.
* **A GC-stress arm.** See "GC" above for why that is a decision rather than an
  omission.

## What P11 did not do

* **It did not touch `perry-stdlib`'s `reqwest`.** That is the surviving group-G
  edge and it serves the declining `fetch` path, `js_fetch_stream_start` and a
  proxied fetch — P8's group A and P6's stream surface.
* **It did not migrate `crates/perry-stdlib/src/axios.rs`**, which is still
  reqwest and still only reachable under `PERRY_DISABLE_WELL_KNOWN=1`.
* **It did not give `perry-ext-axios` the surface it is missing** — request
  headers, `axios.create()`, interceptors, `response.headers`, `AbortSignal`.
  Those are binding features, not transport.
* **It did not remove tokio from `perry-ext-axios`'s *thread*.** The manifest
  edge is gone, but the closure still reaches its pool through
  `perry_ffi::spawn_blocking` → `async_bridge::runtime().spawn_blocking`, which
  is perry-stdlib's tokio. That is P8's group L and it is last by construction.
  It also means an axios call still trips `native_work_inflight` and therefore
  still produces `tokio_ticks > 0` on the park — unchanged from before, and
  worth knowing before anyone reads a counter and concludes axios is still on
  reqwest.
* **It did not fix the alias-name node-fetch defect** (defect 2 above). It is a
  codegen/HIR change and it is outside a transport lane.
* **It did not add a connection pool to `perry-http-client`.** A CLI makes a
  handful of requests spread over minutes of build time, so a kept-alive socket
  would be idle far past any server's timeout. `perry-ext-axios` is the caller
  that would benefit, and it would need the pool to outlive the call — which
  means the loop outliving the call, which is a different crate.

## For the integrator

- The branch is `turnloop/p11-cli-clients` on `origin`. **Nothing here bumps the
  version.** The changelog fragment is
  `changelog.d/turnloop-p11-cli-clients.md`.
- **Three crates change shape.** `crates/perry-http-client` and
  `crates/perry-tls-session` are new; `crates/perry-ext-fetch` is **deleted**.
  Reverting the deletion alone is two rows in
  `crates/perry/well_known_bindings.toml` plus the crate directory — but that
  also restores the SIGSEGV, so revert the whole commit rather than half of it.
- **`Cargo.lock` gains nothing.** No new external crate: `perry-http-client`
  uses `turnloop`, `turnloop-http`, `turnloop-tls`, `url`, `base64` and `sha1`,
  all already resolved. `sha1` is named in `[workspace.dependencies]` for the
  first time, at the 0.11 already in the lockfile through perry-stdlib.
- **`scripts/tokio_inventory.json` is updated in the same commit**, as the gate
  requires. `python3 scripts/tokio_inventory.py` must print **39 edges across 13
  crates**.
- `docs/src/native-libraries/governance.md`'s generated block and
  `workspace-architecture.json`'s baseline are both regenerated; both are gates.
- Run, on a machine with the pinned oracle:

```bash
python3 scripts/tokio_inventory.py            # 39 edges, 13 crates
python3 scripts/tokio_inventory.py --list
cargo test -p perry-http-client               # 34 + 1 doc-test

cargo build --release --locked \
  -p perry -p perry-runtime -p perry-stdlib -p perry-runtime-static -p perry-stdlib-static \
  -p perry-ext-http -p perry-ext-net -p perry-ext-ws -p perry-ext-zlib -p perry-ext-events \
  -p perry-ext-axios
PERRY_SKIP_BUILD=1 ./scripts/run_gap_tests.sh

# needs the network
cargo run --release -p perry-http-client --example probe -- \
  https://example.com/ https://api.github.com/meta http://github.com/
cargo run --release -p perry-http-client --example ws_probe -- wss://echo.websocket.org/
```

- The two trees are on the build box at `/root/claude-turnloop-p11/{base,perry}`
  (base at `1edb5b7e8d`), each with its own `target/`. Delete both when the A/B
  is done. `PERRY_RUNTIME_DIR` must be overridden per tree —
  `/etc/profile.d/perry.sh` points it at a different checkout.
