'use strict'
// Shaped like node-postgres 8.23's lib/crypto/utils.js: a CommonJS helper
// module whose exports share names with Perry's `crypto` intrinsics.
const nodeCrypto = require('crypto')
const webCrypto = nodeCrypto.webcrypto || globalThis.crypto
const subtle = webCrypto.subtle
const enc = new TextEncoder()

async function sha256(data) {
  return await subtle.digest('SHA-256', data)
}
async function hmacSha256(keyBuffer, msg) {
  const key = await subtle.importKey('raw', keyBuffer, { name: 'HMAC', hash: 'SHA-256' }, false, ['sign'])
  return await subtle.sign('HMAC', key, enc.encode(msg))
}
async function deriveKey(password, salt, iterations) {
  const key = await subtle.importKey('raw', enc.encode(password), 'PBKDF2', false, ['deriveBits'])
  return await subtle.deriveBits({ name: 'PBKDF2', hash: 'SHA-256', salt, iterations }, key, 256)
}
function md5(s) {
  return 'user-md5(' + s + ')'
}
function randomBytes(n) {
  return 'user-randomBytes(' + n + ')'
}
module.exports = { sha256, hmacSha256, deriveKey, md5, randomBytes }
