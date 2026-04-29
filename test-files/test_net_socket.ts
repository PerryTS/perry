// Smoke test for the new net.Socket stdlib module (workstream A1.5).
// Connects to a local echo server, sends a known payload, verifies echo,
// then closes cleanly.
//
// Run: start the echo server on 17891 (see test_net_echo_server.rs), then
//   perry compile test_net_socket.ts -o test_net_socket && ./test_net_socket
// Expected output:
//   connected
//   got echo: hello-perry-net
//   closed
//   OK

import { createConnection } from 'net';

const ECHO_HOST = '127.0.0.1';
const ECHO_PORT = 17891;
const PAYLOAD = 'hello-perry-net';

let received = '';
let seen_connect = false;
let seen_close = false;

const sock = createConnection(ECHO_PORT, ECHO_HOST);

sock.on('connect', () => {
    seen_connect = true;
    console.log('connected');
    sock.write(Buffer.from(PAYLOAD, 'utf8'));
});

sock.on('data', (buf: Buffer) => {
    received += buf.toString('utf8');
    if (received.length >= PAYLOAD.length) {
        console.log('got echo: ' + received);
        sock.end();
    }
});

sock.on('close', () => {
    seen_close = true;
    console.log('closed');
    if (seen_connect && received === PAYLOAD) {
        console.log('OK');
    } else {
        console.log('FAIL: connect=' + seen_connect + ' received="' + received + '"');
    }
});

sock.on('error', (err: string) => {
    console.log('ERROR: ' + err);
});

// Keep the event loop alive. Perry's stdlib pump drains net events
// inside js_stdlib_process_pending; without an active tick the main
// thread would exit before the 'close' event fires.
setInterval(() => {
    if (seen_close) {
        // We can exit cleanly once the close event has been dispatched.
        // (process.exit is a Perry stdlib builtin.)
        process.exit(0);
    }
}, 50);
