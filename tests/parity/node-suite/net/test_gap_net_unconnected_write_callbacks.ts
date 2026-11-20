import net from 'node:net';
const s: any = new net.Socket();
s.on('error', (e: any) => console.log('error', e.code, e.message, s.destroyed));
s.on('close', () => console.log('close', s.destroyed, s.writableLength, s.bytesWritten));
s.on('drain', () => console.log('unexpected drain'));
console.log('write', s.write('hi', (e: any) => console.log('callback', e.code, e.message)));
console.log('write2', s.write('x', (e: any) => console.log('callback2', e.code, e.message)));
