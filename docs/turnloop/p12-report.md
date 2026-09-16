# turnloop P12 — a public TLS client for a turnloop socket

## What was missing, and for whom

P5 put TLS **above** the turnloop socket rather than beside it. That is what
made `socket.upgradeToTLS` possible without a descriptor handoff: the handle
keeps carrying bytes and a rustls state machine is installed on top of it. The
server half of that work was public — `install_server_session`, which Perry's
HTTP/2 server scored h2spec 147/147 through — and the client half was not.

`begin_client_upgrade` was `pub(crate)`, took `perry-ext-net`'s own
`TlsClientConfigData` (built by reading JS values off an options object) and
settled a `JsNativeAsyncCompletion`. None of that is reachable from, or
appropriate for, a caller that has no JS promise at the point of upgrade. Two
such callers existed:

* the four database bindings on `perry-db-turnloop`. **Every one of their
  protocol cores already asked for the upgrade** — an `UpgradeTls` event — and
  already had the acknowledgement. The host simply had nothing to give them, so
  all four answered `"… TLS is not available on the turnloop transport"` and the
  client fell back to its tokio driver.
* `http2.connect('https://…')`, which needs **ALPN** decided before it knows
  whether it may speak HTTP/2 at all.

## The shape

`crates/perry-tls-turnloop` is the extracted client. It is deliberately free of
both JS values and rustls types, because `perry-db-turnloop` has neither and a
binding forced to build a `rustls::ClientConfig` would need rustls in its own
manifest.

```rust
let mut transport = TlsClientTransport::connect(&options)?;
let progress      = transport.pump(id);   // sends the ClientHello, then submits ciphertext
let facts         = transport.facts();    // alpn, chain, channel binding
```

Three things in the contract come from the callers rather than from taste.

**ALPN is a parameter, not an extra.** `http2.connect` offers `h2` alone and may
not speak HTTP/2 if the server declines it; a database client offers nothing at
all (a protocol list a server has no opinion about is how a middlebox learns to
have one); `fetch` offers both and switches on the answer. An installer that
could not express all three is exactly how the `pub(crate)` one came to be
shaped for one caller. `TlsFacts::alpn` reports what was selected.

**Channel binding is derived here, not by the caller.** `TlsFacts::channel_binding`
is the RFC 5929 `tls-server-end-point` digest of the **verified leaf**, which is
what PostgreSQL's SCRAM-SHA-256-PLUS binds to. The leaf is only reachable through
the session, so a caller made to fetch the chain itself is a caller that can just
as easily hash an unverified one. `None` means the leaf's signature algorithm has
no defined binding (Ed25519, notably) — the honest answer, and the one that makes
`turnloop-postgres` fall back to plain SCRAM rather than authenticate with a
bogus binding.

**There is one upgrade shape, not four.** The four protocol crates disagree about
*when* TLS begins — `turnloop-redis` and `turnloop-mongodb` raise `UpgradeTls`
from `transport_connected`, before a protocol byte, while `turnloop-postgres`
raises it after the one-byte `S` answer to its `SSLRequest` and `turnloop-mysql`
after the server greeting and its own `SSLRequest` packet — but they agree about
*how they ask*. So `perry-db-turnloop` holds the configuration and lets the core
choose the moment:

```
drain() → take_tls_request() → flush()          ← the plaintext SSLRequest goes out HERE
                             → begin_tls()      ← then the session is installed
   … handshake …            → tls_established(&facts)
```

That order is not interchangeable. Installing first would encrypt the very packet
that asks for encryption, and both mid-stream cores refuse `tls_established`
while their output is unflushed — so the mistake surfaces as a state error rather
than as a hang.

The state machine itself is `perry-tls-session`'s, which gains
`peer_certificates()` and `tls_server_end_point()`. There is still exactly one
copy of it on the client side.

## What moved, and what did not

| surface | before | after |
|---|---|---|
| `pg` with `ssl` | no `ssl` field existed; sqlx has no TLS backend | **turnloop**, `SslMode::Require`, SCRAM-SHA-256-**PLUS** with channel binding |
| `mysql2` with `ssl` | `to_url` hardcoded `?ssl-mode=disabled`; a URI's `?ssl-mode=` was swallowed into the database name | **turnloop**, mid-stream `SSLRequest`; URI query strings parsed |
| `ioredis` `rediss://` | declined — onto a path with no TLS backend, so it **failed**. This is the DEFAULT (`REDIS_TLS` defaults to `true`) | **turnloop**, connect-time |
| `mongodb` `tls=true` | declined to the `mongodb` driver, which really does TLS | **turnloop**, connect-time |
| `http2.connect('https://…')` | cleartext socket to port **80**; `InvalidContentType` | **turnloop** + TLS + `h2` in ALPN |
| `mongodb+srv://`, `replicaSet=`, several hosts, `compressors=`, a `tls*` URI key | legacy | unchanged — SRV and topology discovery are not in scope |
| a Unix-socket `pg`/`mysql2` host | legacy | unchanged — the driver submits a TCP connect |
| any client on a thread with no loop of its own | legacy | unchanged |
| `http2.createServer` / `createSecureServer` on such a thread, or in a cluster worker | `h2` | unchanged |

### The tokio inventory does not move, and here is why

`python3 scripts/tokio_inventory.py` still reports **38 manifest edges**. That is
the honest result, not a shortfall against the brief: the inventory counts
*manifest dependency edges*, and removing one means deleting the legacy arm
entirely. Every group-B edge has a **second, non-TLS reason to decline** — a
thread that could not get a loop of its own; a Unix-domain-socket host for `pg`
and `mysql2`; SRV and replica sets for `mongodb` — and group D's `h2` still
serves `http2.createServer` on those same threads and in cluster workers. As the
group-D entry already said before this lane: *"Removing the edge means deleting
HTTP/2 on those paths, not migrating it."*

What this lane removes is the **blocker**, which is what those entries record.
`scripts/tokio_inventory.json`'s annotations for the nine affected edges are
re-written to say what is actually left, because the file's own rule is that it
must never describe a tree that is gone.

## The result, measured

Every fixture below runs the same TypeScript file twice — once compiled by Perry,
once on the pinned Node 26.5.1 oracle with the real npm package — against the
same TLS-enabled server, and diffs the two outputs byte for byte.
`PERRY_LOOP_STATS=1 PERRY_DB_TURNLOOP_DIAG=1` is on for the Perry arm, so the run
also has to say that turnloop carried it.

### `pg` over TLS, with SCRAM-SHA-256-PLUS

```
$ scripts/turnloop/apps/pg_tls_parity.ts
ssl: command=SELECT rowCount=1 rows=[{"ssl":true}]
select-all: command=SELECT rowCount=2 rows=[{"id":1,"name":"alpha"},{"id":2,"name":"bêta"}]
wide-len: 70000
error-rejected: yes
untrusted-ca-refused: yes
done
=== diff (perry vs node) ===
BYTE-IDENTICAL

--- perry stderr ---
[perry-db] subsystem=2 id=… tls established alpn="" chain=1 channel_binding=true
[perry-pg] scram mechanism=SCRAM-SHA-256-PLUS gs2=p=tls-server-end-point,,
[perry-loop] driver=turnloop turns=45 os_waits=24 completions=53
[perry-loop-waits] arm=turnloop turnloop_waits=44 tokio_ticks=0
```

The PLUS line is not a claim about intent: the mechanism is read off the SCRAM
client-first message that went on the wire, and PostgreSQL **re-computes** the
`tls-server-end-point` digest from its own certificate and compares it inside the
SASL exchange. A run that authenticates at all is the server's verdict on the
binding. Independently cross-checked with `psql "… channel_binding=require"`,
which the same server accepts.

`untrusted-ca-refused: yes` is the control that stops the rest from being
vacuous: the same client, with `rejectUnauthorized` on and no trust material for
the server's private CA, must fail. A TLS client that verifies nothing passes
every other line in that transcript.

### The rest

### `ioredis` over TLS

```
$ scripts/turnloop/apps/redis_tls_parity.ts        # --tls-port 56380
set: OK      get: hello      get-utf8: "héllo\nwörld"
big-len: 70000   big-intact: true   incr: 1   incr2: 2   done
=== diff (perry vs node) ===
BYTE-IDENTICAL

[perry-db] subsystem=5 id=… tls established alpn="" chain=2 channel_binding=true
[perry-db] subsystem=5 closed id=… connects=1 reads=27 writes=16 timer_arms=29 live=0
[perry-loop] driver=turnloop turns=42 completions=81 … tokio_ticks=0
```

This is the row that was **broken**, not merely un-migrated: `REDIS_TLS`
defaults to `true`, so this is what `new Redis()` does out of the box, and it
declined to a legacy path with no TLS backend compiled in.

### `http2.connect('https://…')`

```
$ scripts/turnloop/apps/http2_tls_parity.ts
alpn: h2
encrypted: true
response: 200 path=/hello
done
=== diff (perry vs node) ===
BYTE-IDENTICAL

[perry-loop] driver=turnloop turns=15 completions=33 … tokio_ticks=0
```

`alpn: h2` is the line a cleartext-to-port-80 client could never have produced.
The fixture starts its own `http2.createSecureServer` and connects to it, so the
new client is exercised against the known-good server from the HTTP/2 lane.

### `mongodb` over TLS

The transport works and is proven; the fixture is **not** byte-identical to
Node, and neither is its plaintext twin:

```
$ diff mongo_parity.perry.out mongo_tls_parity.perry.out   → identical
$ diff mongo_parity.node.out  mongo_tls_parity.node.out    → identical
$ diff mongo_tls_parity.node.out mongo_tls_parity.perry.out → 12 lines

[perry-db] subsystem=6 id=… tls established alpn="" chain=2 channel_binding=true
[perry-loop] driver=turnloop … tokio_ticks=0
```

`mongo_tls_parity.ts` is `mongo_parity.ts`'s body with **only** the URI changed,
so the comparison that matters is the one between the two Perry arms, and they
are byte-identical: TLS changes nothing this surface can observe. The remaining
12 lines are a pre-existing divergence in the `mongodb` binding's result shapes
(`insertOne().acknowledged`, `find().toArray()`, `updateOne().modifiedCount`),
reproduced on **this branch's plaintext transport** before any of this lane's
code runs. It is not TLS's and this lane does not fix it, but it does mean
P7's `mongo_parity.ts` is currently red on `turnloop/integration`.

### Unit level

`perry-tls-turnloop`'s `handshake_alpn_and_channel_binding` drives a **real**
rustls handshake in memory — client and server, ciphertext carried between them
as `Vec<u8>` instead of through a socket — and asserts that ALPN selected `h2`,
that the reported leaf is the configured certificate, and that the channel
binding equals a SHA-256 computed outside the crate by `openssl`. It fails if the
handshake does not complete, so a green run is not vacuous.

`perry-ext-ioredis` and `perry-ext-mysql2` each drive their real protocol core
through the upgrade and assert the security property directly: **no protocol
byte — above all no `AUTH` and no MySQL handshake response — may precede the
session**. MySQL's asserts the output is exactly 36 bytes, a 4-byte header plus
the 32-byte `SSLRequest`, with capability bit 11 set.

### The gap suite

837 tests on the pinned Node 26.5.1 oracle, one arm, compared against
`test-parity/gap_snapshot.json`: **805 pass, and not one of the 32 that do not
is attributable to this lane.** Nothing the snapshot records as failing now
passes either, so no entry went stale.

| not passing | how it was classified |
|---|---|
| 20 | already recorded in `gap_snapshot.json`, same status |
| 4 | `compile_fail` — **the auto-optimize cache, not the code**: `target/perry-auto-*` held a `libperry_runtime.a` stamped with the PREVIOUS commit, and the compiler refuses a mismatched stamp. All four (`turnloop_http2_server`, `turnloop_http2_control`, `http2_settings`, `gc_http2_pending_event_callback_rooting`) PASS after `rm -rf target/perry-auto-*`. That they are the HTTP/2 four is a coincidence of nothing — they are the tests whose ext archive the auto-optimize path rebuilds |
| 5 | the **oracle itself** fails them: `backoff_options`, `cron_cronjob`, `dayjs_factory_arg`, `moment_methods`, `ratelimiter_memory` need npm packages a fresh clone does not have, and `node --experimental-strip-types` exits non-zero |
| 3 | pre-existing: `2899_2779_2777_static_helpers`, `disposablestack_2875`, `iterator_prototype_next_patch` reproduce **byte-identically** on a v0.5.1573 build, seven merge trains before this branch |

Two things about how that verdict was reached, because both are traps this
repository has a written rule about and both were walked into here first:

* **`run_parity_tests.sh --filter test_gap_` does not run the snapshot gate.**
  `run_gap_tests.sh` is the wrapper that does. The comparison above was made by
  hand against `gap_snapshot.json` afterwards.
* **The runner's exit code was `tail`'s, not the harness's** —
  `./run_parity_tests.sh … | tail -80` followed by `$?` reports 0 whatever the
  harness did. CLAUDE.md names this exact shape ("check the harness's exit code,
  not a wrapper shell's") and it still got written. The classification above
  comes from the report JSON, not from that exit code.

### Under the GC instruments

`object_field_by_name` is the one thing in this lane that touches the collector,
so the `pg` TLS fixture was re-run on it — `new Client({ …, ssl })` is the call
that reaches it — with the #7154 family armed:

```
PERRY_GC_SCHEDULE_SEED=20260916 PERRY_GC_SCHEDULE_RATE=1 PERRY_GC_SCHEDULE_ALLOC_KB=0 \
PERRY_GC_PROTECT_FROMSPACE=1 PERRY_GC_PROTECT_FROMSPACE_DEPTH=800 \
PERRY_GC_VERIFY_EVACUATION=1
```

Output byte-identical to the unarmed run. 61 forced copying minors, 8 775
objects moved, `[gc-verify] minor=N evacuation_ok` on every one, and the
from-space quarantine armed at `mode=ProtectPages` with no fault.

**With one limit the runtime states itself**, and it is worth repeating rather
than burying: the exit verdict was

> `THIS RUN EXERCISED NOTHING WORTH TRUSTING. … NOT ONE back-edge poll was
> reached, so every collection came from an event-loop boundary and no loop body
> was covered.`

— and the process exits non-zero on that verdict rather than reporting success.
This fixture has no allocating loop codegen emits a poll for. So what is
established is "clean across 61 forced evacuating minors at event-loop
boundaries, with evacuation verification on", not "clean under in-loop
collection". For the call this lane is about that is the relevant window
anyway — `new Client(…)` is at a turn boundary — but the stronger claim is not
made.

## Enabling TLS on the four P7 database servers

P7 brought them up without TLS (`/root/claude-turnloop-p7/dbservers.sh`). The
recipe is `scripts/turnloop/dbservers-tls.sh`, and the shape matters:

| server | port | how TLS is reached |
|---|---|---|
| PostgreSQL 16 | 55432 | `ssl = on` in `postgresql.conf`, **same port** — `SSLRequest` negotiates per connection, so plaintext clients keep working |
| MySQL 8 | 53306 | `--ssl-ca/--ssl-cert/--ssl-key`, **same port** — the SSL capability flag is negotiated mid-handshake |
| Redis 7 | 56379 + **56380** | a *separate* `--tls-port`: Redis has no in-band upgrade |
| MongoDB 8 | 57017 | `--tlsMode preferTLS`, **same port** — accepts both |

Two things that cost time and are worth writing down:

* **MongoDB 8 needs both `--tlsCAFile` and
  `--tlsAllowConnectionsWithoutCertificates`, and neither alone works.** Without
  a CA it refuses to start at all — *"The use of TLS without specifying a chain
  of trust is no longer supported"* (SERVER-72839). With a CA and nothing else it
  **requires a client certificate**, and every client — `mongosh` included — is
  refused with `No SSL certificate provided by peer; connection rejected`. Both
  flags together are the server-authentication-only configuration.
* **The leaf certificates are RSA/SHA-256 on purpose.** RFC 5929 derives the
  `tls-server-end-point` hash from the *signature* algorithm, so an Ed25519 leaf
  makes `turnloop_tls::tls_server_end_point` return `None` — a correct answer
  that would silently turn a SCRAM-SHA-256-**PLUS** proof into a plain SCRAM one.

## Defects found and fixed in passing

**`perry_ffi::object_field_by_name` held an unrooted heap pointer across an
allocation.** It took `*mut ObjectHeader` out of the receiver, *then* called
`alloc_string(key)`, *then* dereferenced it. A moving collection in that window
leaves a stale pointer — the `#7184`/`#7192` shape CLAUDE.md's rooting-invariant
section describes, which presents as a rooted slot holding a dangling pointer and
surfaces cycles later as `TypeError: value is not a function`. The receiver
arrived as an `f64` in a register, which is not a root the collector can see, so
reordering alone would not have been enough: it is now parked in a
`TransientRootScope` and re-read after the allocation. `perry-ext-mysql2` already
had a crate-local version that did this correctly, which is how it was noticed.

**A silent TLS downgrade on both sqlx paths.** `ssl` only became parseable with
this lane, which made a new failure reachable: a client that asked for it and
then *declined* the turnloop transport would have connected in plaintext and sent
its password in the clear. `to_url` now emits `sslmode=verify-full` /
`ssl-mode=REQUIRED`, and this crate's sqlx has no TLS backend, so it answers
*"TLS upgrade required by connect options but SQLx was built without TLS support
enabled"* and refuses. A silent downgrade is the one outcome worse than a refused
connection.

**`perry-ext-http` did not compile on this branch.** The merge that brought
`tcp_listen`'s split `reuse_port` / `noDelay` arguments onto `turnloop/integration`
left the HTTP/2 listener's call site at six arguments. Fixed with the value that
preserves the previous behaviour.

## Not done

* **Client certificates (mTLS).** `turnloop_tls::ClientConfig::new` builds with
  `with_no_client_auth()` and has no constructor that takes a client identity, so
  no turnloop TLS client in Perry can present one — `fetch` and SMTP have the same
  limitation today. A `pg` config carrying `ssl: { cert, key }` connects with
  server authentication only rather than failing, which is the one place this
  lane knowingly diverges from Node. Closing it is an upstream addition to
  `turnloop-tls`, not a change here.
* **`getPeerCertificate()` on a database connection.** The chain is carried in
  `TlsFacts` and thrown away; no binding has a JS surface for it.
* **`http2.connect`'s remaining TLS options** — `servername`, `cert`/`key`,
  `checkServerIdentity`. `ca` and `rejectUnauthorized` are read; the rest are
  ignored rather than refused.
* **The protocol version and cipher suite** are not reported anywhere.
  `turnloop_tls`'s `Client` does not expose `protocol_version()`, so
  `socket.getProtocol()`-shaped surfaces would need an upstream accessor.
