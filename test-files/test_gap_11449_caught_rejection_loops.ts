// Consecutive rejected awaits are valid progress, including across async calls.
async function countRejections(n: number, reason: any) {
  let caught = 0;
  let matched = 0;
  for (let i = 0; i < n; i++) {
    try {
      await Promise.reject(reason);
    } catch (e) {
      caught++;
      if (e === reason) matched++;
    }
  }
  return caught + ':' + matched;
}

async function missingMethod() {
  const object: any = {};
  let caught = 0;
  try {
    await object.missing();
  } catch (_) {
    caught++;
  }
  console.log('missing method caught', caught);
}

async function main() {
  await countRejections(1000, 'warm-up');
  console.log('number', await countRejections(12050, 7));
  console.log('object', await countRejections(12050, { limited: true }));
  console.log('error', await countRejections(12050, new Error('expected')));
  await missingMethod();
  // Retain real exception propagation after a long run of handled rejections.
  try {
    await Promise.reject('final');
  } catch (e) {
    console.log('final', e);
  }
}
main().catch((e) => { console.log('unexpected', e.message); process.exitCode = 1; });
