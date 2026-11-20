'use strict'
// Shaped like node-postgres 8.23's lib/crypto/sasl.js: the helper module is
// bound to the name `crypto`, which Perry used to read as the builtin.
const crypto = require('./scram_utils.cjs')

function xorBuffers(a, b) {
  return Buffer.from(a.map((_, i) => a[i] ^ b[i]))
}

async function clientFinal(password, clientNonce, serverFirst) {
  const attrs = new Map(serverFirst.split(',').map((kv) => [kv[0], kv.substring(2)]))
  const nonce = attrs.get('r')
  const salt = attrs.get('s')
  const iterations = parseInt(attrs.get('i'), 10)
  const clientFinalWithoutProof = 'c=biws,r=' + nonce
  const authMessage = 'n=*,r=' + clientNonce + ',' + serverFirst + ',' + clientFinalWithoutProof
  const salted = await crypto.deriveKey(password, Buffer.from(salt, 'base64'), iterations)
  const clientKey = await crypto.hmacSha256(salted, 'Client Key')
  const storedKey = await crypto.sha256(clientKey)
  const clientSignature = await crypto.hmacSha256(storedKey, authMessage)
  const proof = xorBuffers(Buffer.from(clientKey), Buffer.from(clientSignature)).toString('base64')
  const serverKey = await crypto.hmacSha256(salted, 'Server Key')
  const serverSignature = Buffer.from(await crypto.hmacSha256(serverKey, authMessage)).toString('base64')
  return {
    storedKey: Buffer.from(storedKey).toString('hex'),
    response: clientFinalWithoutProof + ',p=' + proof,
    serverSignature,
  }
}

function otherMethods() {
  return [crypto.md5('a'), crypto.randomBytes(3)].join(' ')
}

module.exports = { clientFinal, otherMethods }
