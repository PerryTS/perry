// Issue #422 regression: `new net.Socket()` + `sock.connect(port, host)`
// must fire `'connect'` or `'error'` events, and `net.connect(port, host)`
// must return a Socket (not undefined).
//
// Pre-fix symptom (v0.5.500 and earlier):
//   - reproducer 1: `new net.Socket()` produced an empty-object placeholder;
//     `.connect/.on/.write` all silently no-op'd, and 'connect' / 'error'
//     never fired. The TCP socket was never opened (lsof showed nothing).
//   - reproducer 3: `net.connect(port, host)` returned the literal
//     undefined sentinel.
//
// We connect to 127.0.0.1:1 — port 1 (tcpmux) is reserved and almost never
// has a real listener on a normal host, so the connect attempt fires
// `'error'` (ECONNREFUSED) within the kernel's TCP retry window. That's
// deterministic — pre-fix it never fired AT ALL, post-fix it fires within
// a few ms. Either firing proves the dispatch is wired; we don't depend
// on a specific error message because OS strings vary.
//
// Output is pinned via test-parity/expected/ rather than running through
// the byte-for-byte Node parity check because Node's `net` formatting
// (`Socket { ... internal fields ... }`) wouldn't match perry's anyway,
// and the meat of this test is "did the dispatch fire at all".

import * as net from "net";

let done = false;

// ── Reproducer 1: new net.Socket() then sock.connect(port, host) ───────
const sock = new net.Socket();
console.log("[1] socket allocated, type:", typeof sock);

let r1Connect = 0;
let r1Error = 0;

sock.on("connect", () => {
    r1Connect++;
});
sock.on("error", (_e: string) => {
    r1Error++;
});

sock.connect(1, "127.0.0.1");
console.log("[2] connect call returned");

// ── Reproducer 3: net.connect(port, host) returns a Socket ─────────────
const sock2 = net.connect(1, "127.0.0.1");
console.log("[3] net.connect typeof:", typeof sock2);

let r3Error = 0;
sock2.on("error", (_e: string) => {
    r3Error++;
});

// Drain pending events for up to ~1.5s, then report.
let ticks = 0;
const tid = setInterval(() => {
    ticks++;
    if (r1Error + r1Connect > 0 && r3Error > 0) {
        // Both sockets observed at least one event — fix is working.
        finish();
    } else if (ticks >= 30) {
        // 30 × 50ms = 1.5s — enough for ECONNREFUSED on any sane host.
        finish();
    }
}, 50);

function finish() {
    if (done) return;
    done = true;
    clearInterval(tid);
    console.log("[4] r1 connect+error events fired:", r1Connect + r1Error > 0);
    console.log("[5] r3 connect+error events fired:", r3Error > 0);
    sock.destroy();
    sock2.destroy();
    process.exit(0);
}
