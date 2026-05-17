import { Readable } from 'node:stream';
const r = new Readable({ read() { this.push('hi'); this.push(null); } });
let collected = '';
r.on('data', (chunk: any) => { collected += chunk.toString(); });
r.on('end', () => { console.log('END:', collected); });
