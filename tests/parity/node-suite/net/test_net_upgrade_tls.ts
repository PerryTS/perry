// A2 smoke test: plain TCP → upgradeToTLS swap on the same socket.
// Exercises the Postgres-style SSLRequest flow primitive.
//
// Companion server: test_net_upgrade_tls_server.py listens plain, reads
// a 1-byte marker, responds 'S' (like Postgres ServerSupportsSSL), then
// wraps its own socket in TLS and echoes one message.

import * as net from 'net';

const HOST = '127.0.0.1';
const PORT = 17892;
const PAYLOAD = 'hello-after-upgrade';

let received = '';
let done = false;
let upgraded = false;

const sock = net.createConnection(PORT, HOST);

sock.on('connect', () => {
    console.log('plain connect ok');
    // Mimic Postgres SSLRequest — send one byte, server replies 'S'.
    sock.write(Buffer.from([0x53])); // 'S'
});

sock.on('data', async (buf: Buffer) => {
    if (!upgraded) {
        const reply = buf.toString('utf8');
        console.log('server negotiation byte: ' + reply);
        if (reply === 'S') {
            upgraded = true;
            console.log('upgrading to TLS...');
            await sock.upgradeToTLS(HOST, 0); // verify=0: self-signed OK for this test
            console.log('tls upgrade ok');
            sock.write(Buffer.from(PAYLOAD, 'utf8'));
        }
    } else {
        received += buf.toString('utf8');
        if (received.length >= PAYLOAD.length) {
            console.log('echo over TLS: ' + received);
            sock.end();
        }
    }
});

sock.on('close', () => {
    done = true;
    if (received === PAYLOAD) {
        console.log('OK');
    } else {
        console.log('FAIL (received=' + received + ')');
    }
});

sock.on('error', (e: string) => {
    console.log('ERROR: ' + e);
});

setInterval(() => { if (done) process.exit(0); }, 50);
