import { webcrypto } from 'node:crypto';
const subtle = webcrypto.subtle;
const input = new TextEncoder().encode('test');
const key = await subtle.importKey('raw', input, { name: 'HMAC', hash: 'SHA-256' }, false, ['sign', 'verify']);
const signature = await subtle.sign('HMAC', key, input);
console.log(key.type, signature.byteLength, await subtle.verify('HMAC', key, signature, input));
