// Regression test for issue #647: AOT-compiled `await` of a Promise
// resolved from a `net.Socket` event listener used to wedge subsequent
// `'data'` events on the same socket.
//
// Before the fix in v0.5.772, `s.on('data', cb)` and `s.write(buf)`
// at the top level of an async function silently no-op'd because the
// HIR pass `js_transform.rs::detect_native_instance_creation_with_context`
// blanket-matched ANY `createConnection` method (regardless of module)
// to class_name "Connection". `net.createConnection(...)` got mis-tagged
// as ("net", "Connection") instead of ("net", "Socket"), so the static
// dispatch table's `class_filter: Some("Socket")` row missed and the
// "Unknown native method" fallback returned 0.0 without calling
// `js_net_socket_*`. The fix made the lookup module-aware.
//
// Requires the python echo server at 127.0.0.1:17891 (run_parity_tests.sh
// spawns it before invoking gap tests).

import { createConnection } from 'net';

async function main(): Promise<void> {
    const s = createConnection(17891 as never, '127.0.0.1' as never);

    s.on('data', (b: Buffer) => {
        console.log('got', b.length, JSON.stringify(b.toString('utf8')));
        s.end();
        process.exit(0);
    });

    await new Promise<void>((resolve) => {
        s.on('connect', () => resolve());
    });

    console.log('connected, writing');
    s.write(Buffer.from('hello-647'));
    setTimeout(() => { console.log('TIMEOUT'); process.exit(2); }, 2000);
}
main();
