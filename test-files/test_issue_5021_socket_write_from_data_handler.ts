// Regression test for issue #5021: `net.Socket.write()` called from INSIDE a
// `'data'` event handler silently dropped the bytes on a Perry-native Linux
// binary — no `write()` syscall fired and the peer never saw the data.
//
// Root cause: a socket created through perry-ext-net (the well-known-flip net
// implementation) registers in ext-net's socket map, but the runtime's
// HANDLE_METHOD_DISPATCH fallback for a captured-by-closure `s.write(...)`
// routed through the SHARED `js_net_socket_write` symbol. In a build that also
// links the bundled stdlib net (jsruntime path), that symbol bound to the
// bundled twin's EMPTY registry, so `sockets.get(&handle)` missed and the
// `SocketCommand::Write` was never enqueued — the bytes vanished. Fixed by
// routing write/end/destroy through perry-ext-net's DISTINCT, twin-free
// `js_ext_net_*` symbols (mirrors the #5010 `js_ext_net_destroy_socket` fix).
//
// The write here is issued from within the `'data'` handler (the failing
// shape — DB drivers read a greeting, then write the auth response back from
// their data callback). Requires the python echo server at 127.0.0.1:17891
// (run_parity_tests.sh spawns it before invoking these tests).

import { createConnection } from 'net';

const s = createConnection(17891 as never, '127.0.0.1' as never);
let phase = 0;

s.on('connect', () => {
    // Kick off the exchange: the echo server replies with whatever we send,
    // which gives us a 'data' event to write back from.
    s.write(Buffer.from('PING'));
});

s.on('data', (b: Buffer) => {
    phase++;
    if (phase === 1) {
        // First echo received. Now issue a write FROM INSIDE the data handler
        // — the exact path that #5021 dropped on the floor.
        s.write(Buffer.from('FROM-DATA-HANDLER'));
    } else {
        // If the write-from-data-handler reached the wire, the echo server
        // bounced it back here.
        console.log('got', JSON.stringify(b.toString('utf8')));
        s.end();
        process.exit(0);
    }
});

s.on('error', (e) => {
    console.log('ERROR', String(e));
    process.exit(1);
});

setTimeout(() => {
    // Pre-fix: the second write never reached the server, so the second
    // 'data' event never fired and we time out here.
    console.log('TIMEOUT');
    process.exit(2);
}, 3000);
