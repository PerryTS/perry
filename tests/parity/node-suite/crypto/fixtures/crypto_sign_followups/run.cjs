// #11447 follow-ups: the CommonJS `require('crypto')` receiver (jwa's shape)
// for one-shot sign/verify, every Node digest spelling, and ECDSA on the
// curves jsonwebtoken's ES384/ES512 require.
const crypto = require('crypto');
const keys = require('./keys.cjs');

function digestName(bits) {
  return 'RSA-SHA' + bits;
}

function oneShot() {
  const data = Buffer.from('header.payload');
  for (const bits of ['1', '224', '256', '384', '512']) {
    const signer = crypto.createSign(digestName(bits));
    signer.update(data);
    const viaHandle = signer.sign(keys.RSA_PRIVATE, 'hex');
    const viaOneShot = crypto.sign(digestName(bits), data, keys.RSA_PRIVATE);
    console.log('rsa', bits, viaHandle.slice(0, 24), viaOneShot.toString('hex') === viaHandle,
      crypto.verify(digestName(bits), data, keys.RSA_PUBLIC, viaOneShot),
      crypto.verify(digestName(bits), Buffer.from('tampered'), keys.RSA_PUBLIC, viaOneShot));
  }
  crypto.sign('sha256', data, keys.RSA_PRIVATE, (err, sig) => {
    console.log('callback sign', err, sig.length);
    crypto.verify('sha256', data, keys.RSA_PUBLIC, sig, (err2, ok) => console.log('callback verify', err2, ok));
  });
}

function spellings() {
  const names = ['sha1', 'SHA1', 'RSA-SHA1', 'RSA-SHA1-2', 'sha1WithRSAEncryption',
    'sha224', 'SHA224', 'RSA-SHA224', 'sha224WithRSAEncryption',
    'sha256', 'SHA256', 'RSA-SHA256', 'sha384', 'SHA512', 'sha512WithRSAEncryption'];
  for (const name of names) {
    const s = crypto.createSign(name);
    s.update('abc');
    const sig = s.sign(keys.RSA_PRIVATE);
    const v = crypto.createVerify(name);
    v.update('abc');
    console.log('alias', name, sig.toString('hex').slice(0, 16), v.verify(keys.RSA_PUBLIC, sig));
  }
  const pss = { key: keys.RSA_PRIVATE, padding: crypto.constants.RSA_PKCS1_PSS_PADDING, saltLength: 20 };
  const pssPub = { key: keys.RSA_PUBLIC, padding: crypto.constants.RSA_PKCS1_PSS_PADDING, saltLength: 20 };
  const sig = crypto.sign('sha1', Buffer.from('abc'), pss);
  console.log('pss sha1', sig.length, crypto.verify('sha1', Buffer.from('abc'), pssPub, sig));
}

function ec(label, privatePem, publicPem, digest, p1363Len) {
  const priv = crypto.createPrivateKey(privatePem);
  const pub = crypto.createPublicKey(privatePem);
  console.log(label, priv.type, priv.asymmetricKeyType, priv.asymmetricKeyDetails.namedCurve,
    pub.type, pub.asymmetricKeyDetails.namedCurve,
    crypto.createPublicKey(publicPem).asymmetricKeyDetails.namedCurve);
  const signer = crypto.createSign(digest);
  signer.update('header.payload');
  const der = signer.sign(privatePem);
  const verifier = crypto.createVerify(digest);
  verifier.update('header.payload');
  const tampered = crypto.createVerify(digest);
  tampered.update('header.payloaD');
  console.log(label, 'der', der[0] === 0x30, verifier.verify(publicPem, der), tampered.verify(publicPem, der));
  const raw = crypto.sign(digest, Buffer.from('x'), { key: privatePem, dsaEncoding: 'ieee-p1363' });
  console.log(label, 'p1363', raw.length === p1363Len,
    crypto.verify(digest, Buffer.from('x'), { key: publicPem, dsaEncoding: 'ieee-p1363' }, raw),
    crypto.verify(digest, Buffer.from('x'), { key: pub, dsaEncoding: 'ieee-p1363' }, raw));
}

function generated(curve, digest) {
  const pair = crypto.generateKeyPairSync('ec', {
    namedCurve: curve,
    publicKeyEncoding: { type: 'spki', format: 'pem' },
    privateKeyEncoding: { type: 'pkcs8', format: 'pem' },
  });
  const sig = crypto.sign(digest, Buffer.from('gen'), pair.privateKey);
  console.log('generated', curve, crypto.createPrivateKey(pair.privateKey).asymmetricKeyDetails.namedCurve,
    crypto.verify(digest, Buffer.from('gen'), pair.publicKey, sig));
}

module.exports = function run() {
  oneShot();
  spellings();
  ec('p384', keys.P384_PRIVATE, keys.P384_PUBLIC, 'sha384', 96);
  ec('p521', keys.P521_PRIVATE, keys.P521_PUBLIC, 'sha512', 132);
  ec('p256-sec1', keys.P256_SEC1_PRIVATE, keys.P256_SEC1_PUBLIC, 'sha256', 64);
  const s = crypto.createSign('sha384');
  s.update('m');
  const wrongCurve = s.sign(keys.P384_PRIVATE);
  const v = crypto.createVerify('sha384');
  v.update('m');
  console.log('cross-curve', v.verify(keys.P521_PUBLIC, wrongCurve));
  // A digest shorter than half the field: OpenSSL signs it as a plain integer.
  for (const [digest, priv, pub] of [['sha256', keys.P521_PRIVATE, keys.P521_PUBLIC],
    ['sha1', keys.P384_PRIVATE, keys.P384_PUBLIC], ['sha1', keys.P521_PRIVATE, keys.P521_PUBLIC]]) {
    const sig = crypto.sign(digest, Buffer.from('short'), priv);
    console.log('short digest', digest, crypto.verify(digest, Buffer.from('short'), pub, sig));
  }
  generated('secp384r1', 'sha384');
  generated('P-521', 'sha512');
};
