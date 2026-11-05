// Test missing crypto and buffer features for Perry parity gap analysis
// Expected: all assertions pass under node --experimental-strip-types

import * as crypto from 'crypto';

// --- crypto.createHash('sha256') ---
const hash = crypto.createHash('sha256').update('hello').digest('hex');
console.log(hash); // 2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824
console.log(hash.length === 64); // true

// SHA256 of empty string
const emptyHash = crypto.createHash('sha256').update('').digest('hex');
console.log(emptyHash); // e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855

// MD5
const md5 = crypto.createHash('md5').update('hello').digest('hex');
console.log(md5); // 5d41402abc4b2a76b9719d911017c592

// --- crypto.createHmac ---
const hmac = crypto.createHmac('sha256', 'key').update('hello').digest('hex');
console.log(typeof hmac === 'string'); // true
console.log(hmac.length === 64); // true

// Verify deterministic
const hmac2 = crypto.createHmac('sha256', 'key').update('hello').digest('hex');
console.log(hmac === hmac2); // true

// --- crypto.getRandomValues ---
const randomBuf = new Uint8Array(16);
crypto.getRandomValues(randomBuf);
console.log(randomBuf.length === 16); // true
// Check that at least some bytes are non-zero (probability of all zeros is 2^-128)
let hasNonZero = false;
for (let i = 0; i < randomBuf.length; i++) {
  if (randomBuf[i] !== 0) {
    hasNonZero = true;
    break;
  }
}
console.log(hasNonZero); // true

// Two calls should produce different results
const randomBuf2 = new Uint8Array(16);
crypto.getRandomValues(randomBuf2);
let allSame = true;
for (let i = 0; i < 16; i++) {
  if (randomBuf[i] !== randomBuf2[i]) {
    allSame = false;
    break;
  }
}
console.log(!allSame); // true (different random values)

// --- Buffer.compare ---
const buf1 = Buffer.from('abc');
const buf2 = Buffer.from('abc');
const buf3 = Buffer.from('abd');
const buf4 = Buffer.from('abb');
console.log(Buffer.compare(buf1, buf2) === 0); // true (equal)
console.log(Buffer.compare(buf1, buf3) < 0); // true (abc < abd)
console.log(Buffer.compare(buf1, buf4) > 0); // true (abc > abb)

// --- buf.readUInt8 / buf.readUInt16BE / buf.readInt32LE ---
const readBuf = Buffer.from([0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08]);
console.log(readBuf.readUInt8(0)); // 1
console.log(readBuf.readUInt8(1)); // 2
console.log(readBuf.readUInt16BE(0)); // 258 (0x0102)
console.log(readBuf.readUInt16BE(2)); // 772 (0x0304)
console.log(readBuf.readInt32LE(0)); // 67305985 (0x04030201 in little-endian)

// --- buf.writeUInt8 / buf.writeUInt16BE ---
const writeBuf = Buffer.alloc(8);
writeBuf.writeUInt8(0xFF, 0);
console.log(writeBuf.readUInt8(0)); // 255
writeBuf.writeUInt16BE(0x1234, 2);
console.log(writeBuf.readUInt16BE(2)); // 4660 (0x1234)

// --- buf.indexOf ---
const searchBuf = Buffer.from('hello world');
console.log(searchBuf.indexOf('world')); // 6
console.log(searchBuf.indexOf('xyz')); // -1
console.log(searchBuf.indexOf(Buffer.from('llo'))); // 2

// --- buf.includes ---
console.log(searchBuf.includes('hello')); // true
console.log(searchBuf.includes('xyz')); // false
console.log(searchBuf.includes(Buffer.from('world'))); // true

// --- buf.swap16 ---
const swapBuf = Buffer.from([0x01, 0x02, 0x03, 0x04]);
swapBuf.swap16();
console.log(swapBuf[0]); // 2
console.log(swapBuf[1]); // 1
console.log(swapBuf[2]); // 4
console.log(swapBuf[3]); // 3

// --- buf.readBigInt64BE ---
const bigBuf = Buffer.alloc(8);
bigBuf.writeBigInt64BE(1234567890123456789n, 0);
const bigVal = bigBuf.readBigInt64BE(0);
console.log(bigVal === 1234567890123456789n); // true

// --- Buffer.concat ---
const concatResult = Buffer.concat([Buffer.from('hello'), Buffer.from(' '), Buffer.from('world')]);
console.log(concatResult.toString()); // hello world

// --- Buffer.alloc with fill ---
const filledBuf = Buffer.alloc(4, 0xAB);
console.log(filledBuf[0]); // 171 (0xAB)
console.log(filledBuf[3]); // 171 (0xAB)

// --- crypto.randomBytes ---
const rBytes = crypto.randomBytes(32);
console.log(rBytes.length === 32); // true
let rHasNonZero = false;
for (let i = 0; i < rBytes.length; i++) {
  if (rBytes[i] !== 0) {
    rHasNonZero = true;
    break;
  }
}
console.log(rHasNonZero); // true

// --- crypto.randomUUID ---
const uuid = crypto.randomUUID();
console.log(typeof uuid === 'string'); // true
console.log(uuid.length === 36); // true
// UUID v4 format: 8-4-4-4-12
console.log(/^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/.test(uuid)); // true

console.log("All crypto/buffer gap tests passed!");

// Expected output:
// 2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824
// true
// e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855
// 5d41402abc4b2a76b9719d911017c592
// true
// true
// true
// true
// true
// true
// true
// true
// true
// 1
// 2
// 258
// 772
// 67305985
// 255
// 4660
// 6
// -1
// 2
// true
// false
// true
// 2
// 1
// 4
// 3
// true
// hello world
// 171
// 171
// true
// true
// true
// true
// true
// All crypto/buffer gap tests passed!
