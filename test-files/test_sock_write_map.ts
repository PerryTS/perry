// Regression test for issue #91: sock.write() via Map-retrieved object
// silently drops packets inside a 'data' callback.
//
// Root cause (fixed in v0.5.145): when `entry.sock` is a property access on
// a Map-retrieved value the HIR can't statically tag the receiver type, so
// the call falls through to `js_native_call_method` → `HANDLE_METHOD_DISPATCH`
// → `dispatch_net_socket` rather than the static NATIVE_MODULE_TABLE path.
// Without the `dispatch_net_socket` handler the write was a silent no-op.
//
// Run against the echo server at port 17891:
//   cargo run -p perry-net-echo-server &
//   perry compile test_sock_write_map.ts -o test_sock_write_map && ./test_sock_write_map
//
// Expected output:
//   connected
//   phase0 ok: direct small write works
//   phase1 ok: map large write works
//   OK

import * as net from 'node:net';

const ECHO_HOST = '127.0.0.1';
const ECHO_PORT = 17891;

const SMALL_PAYLOAD = 'ping';                          // 4 bytes — was never failing
const LARGE_PAYLOAD = 'hello-#91-' + 'x'.repeat(91);  // 101 bytes — was silently dropped

// Map storing { sock } — the struct shape that triggered the bug.
const CONN_MAP = new Map<number, { sock: net.Socket }>();

let phase = 0;
let done = false;

const sock = net.createConnection(ECHO_PORT, ECHO_HOST);
CONN_MAP.set(1, { sock });

sock.on('connect', () => {
    console.log('connected');
    // Phase 0: send small payload via the closure-captured socket (always
    // worked — used as a baseline to confirm the echo server is live).
    sock.write(Buffer.from(SMALL_PAYLOAD, 'utf8'));
});

sock.on('data', (buf: Buffer) => {
    const s = buf.toString('utf8');
    if (phase === 0) {
        if (s !== SMALL_PAYLOAD) {
            console.log('FAIL phase0: got "' + s + '"');
            done = true;
            sock.end();
            return;
        }
        console.log('phase0 ok: direct small write works');
        phase = 1;

        // Phase 1: write the large payload via the Map-retrieved socket.
        // This is the exact pattern that triggered issue #91.
        const entry = CONN_MAP.get(1);
        if (entry !== undefined) {
            entry.sock.write(Buffer.from(LARGE_PAYLOAD, 'utf8'));
        } else {
            console.log('FAIL: entry not found in map');
            done = true;
            sock.end();
        }
    } else if (phase === 1) {
        if (s === LARGE_PAYLOAD) {
            console.log('phase1 ok: map large write works');
            console.log('OK');
        } else {
            console.log('FAIL phase1: got ' + s.length + ' bytes, expected ' + LARGE_PAYLOAD.length);
        }
        done = true;
        sock.end();
    }
});

sock.on('close', () => {
    process.exit(0);
});

sock.on('error', (err: string) => {
    console.log('ERROR: ' + err);
    process.exit(1);
});

setTimeout(() => {
    if (!done) {
        console.log('TIMEOUT: bytes never arrived via Map path (issue #91 regression)');
    }
    process.exit(1);
}, 6000);
