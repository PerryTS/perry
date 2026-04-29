// A2 smoke test: tls.connect against a real HTTPS endpoint.
// Sends a raw HTTP/1.1 GET and verifies we receive a 200 response.
// Proves the full stdlib TLS path: rustls + rustls-native-certs + handshake +
// encrypted read/write through the Transport enum.

import * as tls from 'tls';

const HOST = 'github.com';
const PORT = 443;

console.log('starting');

const sock = tls.connect(HOST, PORT, HOST, 1); // verify=full
let received = '';
let done = false;

sock.on('connect', () => {
    console.log('tls handshake ok');
    const req = 'GET / HTTP/1.1\r\nHost: ' + HOST + '\r\nConnection: close\r\n\r\n';
    sock.write(Buffer.from(req, 'utf8'));
});

sock.on('data', (buf: Buffer) => {
    received += buf.toString('utf8');
});

sock.on('close', () => {
    done = true;
    const firstLine = received.split('\r\n')[0];
    console.log('first line: ' + firstLine);
    if (firstLine.indexOf('200') >= 0) {
        console.log('OK');
    } else {
        console.log('FAIL (no 200 in: ' + firstLine + ')');
    }
});

sock.on('error', (e: string) => {
    console.log('ERROR: ' + e);
});

setInterval(() => {
    if (done) process.exit(0);
}, 50);
