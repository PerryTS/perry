// Minimal diagnostic: does console.log even fire? Does createConnection run?
console.log('start');

import * as net from 'net';

console.log('importing net ok');

const sock = net.createConnection(17891, '127.0.0.1');

console.log('createConnection returned');

sock.on('connect', () => {
    console.log('connect fired');
});
sock.on('error', (e: string) => {
    console.log('error: ' + e);
});

console.log('listeners registered');

// Exit after 2 seconds no matter what so we don't hang forever.
let ticks = 0;
setInterval(() => {
    ticks++;
    console.log('tick ' + ticks);
    if (ticks >= 4) {
        process.exit(0);
    }
}, 500);
