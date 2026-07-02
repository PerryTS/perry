// Regression test for issue #5540: `socket.write()` reached through a property
// of a Map-stored object is silently dropped (no `sendto`, bytes never hit the
// wire) when the write is issued from a TIMER (`setInterval`) callback on a
// Perry-native Linux binary. The identical write through a top-level `const`
// socket from the same timer context worked, and the same Map-retrieved write
// from a `'data'` handler worked (#91) — so the failing shape is specifically
// the COMBINATION of (Map-retrieved object property receiver) + (timer
// callback). It blocked `@perryts/mysql`: its flush pump writes via
// `CONN_STATES.get(id).sock.write(bytes)` from a `setInterval`, so password
// auth timed out.
//
// Root cause was the dynamic property-get → bound-method → call path for an
// `any`-typed `entry.sock.write(...)` receiver. When `net` routes through
// perry-ext-net (the production path), resolving `.write` as a value off the
// socket handle goes through `js_ext_net_handle_property_dispatch`, which binds
// the handle method via the twin-free `js_ext_net_socket_write` symbol so the
// enqueued write reaches ext-net's own registry instead of a bundled twin's
// empty one (mirrors #5021 / #5010). This test locks in that the timer-callback
// Map-write path actually reaches the wire.
//
// Run against the echo server at port 17891 (run_parity_tests.sh spawns it):
//   python3 test-files/test_net_echo_server.py &
//   perry compile test_issue_5540_sock_write_map_from_timer.ts -o t && ./t

import * as net from 'node:net';

const ECHO_HOST = '127.0.0.1';
const ECHO_PORT = 17891;

const SMALL_PAYLOAD = 'ping';                          // 4 bytes — priming write
const LARGE_PAYLOAD = 'hello-#5540-' + 'x'.repeat(93); // 105 bytes — the #5540 write

// Map storing `{ sock }` with an `any`-typed socket field — the exact shape a
// connection-state registry uses, and the one that forces the dynamic dispatch
// path (codegen cannot statically tag the receiver as a net.Socket).
interface St { sock: any; }
const CONN_MAP = new Map<number, St>();

let phase = 0;
let done = false;
let passed = false;
// Accumulate echoed bytes per phase: TCP gives no 1-write-to-1-'data'
// guarantee, so a fragmented (or coalesced) echo must not be mistaken for a
// mismatch. We only advance / succeed once the *full* expected payload has
// arrived, which is what actually proves the round-trip (mirrors #5021).
let phase0Echo = '';
let phase1Echo = '';

const sock = net.createConnection(ECHO_PORT, ECHO_HOST);
CONN_MAP.set(1, { sock });

// The #5540 write: reached through a Map-retrieved object property AND issued
// from inside a setInterval callback. Factored into a top-level function the
// same way @perryts/mysql's flush pump is.
function flush(id: number): void {
    const st = CONN_MAP.get(id);
    if (st === undefined) {
        console.log('FAIL: entry not found in map');
        done = true;
        sock.end();
        return;
    }
    st.sock.write(Buffer.from(LARGE_PAYLOAD, 'utf8'));
}

sock.on('connect', () => {
    console.log('connected');
    // Priming write via the closure-captured const socket (the baseline that
    // always worked) — gives us an echo to drive the timer from.
    sock.write(Buffer.from(SMALL_PAYLOAD, 'utf8'));
});

sock.on('data', (buf: Buffer) => {
    const chunk = buf.toString('utf8');
    if (phase === 0) {
        phase0Echo += chunk;
        if (phase0Echo !== SMALL_PAYLOAD.slice(0, phase0Echo.length)) {
            console.log('FAIL phase0: got "' + phase0Echo + '"');
            done = true;
            sock.end();
            return;
        }
        if (phase0Echo === SMALL_PAYLOAD) {
            console.log('phase0 ok: priming echo received');
            phase = 1;
            // Kick a one-shot setInterval pump that performs the Map-retrieved
            // write. This is the exact pattern #5540 dropped on the floor.
            const iv = setInterval(() => {
                clearInterval(iv);
                flush(1);
            }, 0);
        }
        return;
    }

    // phase 1: the bounce of the Map-retrieved write-from-timer.
    phase1Echo += chunk;
    if (phase1Echo !== LARGE_PAYLOAD.slice(0, phase1Echo.length)) {
        console.log('FAIL phase1: mismatch after ' + phase1Echo.length + ' bytes');
        done = true;
        sock.end();
        return;
    }
    if (phase1Echo === LARGE_PAYLOAD) {
        console.log('phase1 ok: map write from timer reached the wire');
        console.log('OK');
        passed = true;
        done = true;
        sock.end();
    }
});

sock.on('close', () => {
    // Only exit 0 once the OK path has been reached — every FAIL branch also
    // calls sock.end() and lands here, so an unconditional exit(0) would mask
    // a real failure.
    process.exit(passed ? 0 : 1);
});

sock.on('error', (err: string) => {
    console.log('ERROR: ' + err);
    process.exit(1);
});

setTimeout(() => {
    if (!done) {
        console.log('TIMEOUT: bytes never arrived via Map+timer path (issue #5540 regression)');
    }
    process.exit(1);
}, 6000);
