const crypto = require('crypto');

function createKeySigner(bits) {
  return function sign(message, privateKey) {
    const signer = crypto.createSign('RSA-SHA' + bits);
    console.log('signer:', typeof signer);
    signer.update(message);
    return signer.sign(privateKey, 'base64');
  };
}

module.exports = function run() {
  const pair = crypto.generateKeyPairSync('rsa', { modulusLength: 2048 });
  const privateKey = pair.privateKey.export({ type: 'pkcs1', format: 'pem' });
  const publicKey = pair.publicKey.export({ type: 'spki', format: 'pem' });
  for (const bits of ['256', '384', '512']) {
    const signature = createKeySigner(bits)('a.b', privateKey);
    console.log('sig b64 len:', signature.length);
    const verifier = crypto.createVerify('RSA-SHA' + bits);
    verifier.update('a.b');
    console.log('verified:', verifier.verify(publicKey, signature, 'base64'));
  }
  const detachedSign = crypto.createSign;
  const detachedVerify = crypto.createVerify;
  const signer = detachedSign('sha256');
  signer.update('detached');
  const signature = signer.sign(privateKey, 'base64');
  const verifier = detachedVerify('sha256');
  verifier.update('detached');
  console.log('detached:', verifier.verify(publicKey, signature, 'base64'));
  const legacySign = crypto.Sign;
  const legacyVerify = crypto.Verify;
  const legacy = legacySign('sha256');
  legacy.update('legacy');
  const legacySignature = legacy.sign(privateKey, 'base64');
  const checker = legacyVerify('sha256');
  checker.update('legacy');
  console.log('legacy:', checker.verify(publicKey, legacySignature, 'base64'));
};
