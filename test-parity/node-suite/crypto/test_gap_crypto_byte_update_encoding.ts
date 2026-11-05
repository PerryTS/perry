import { createHash, createHmac, hash } from 'node:crypto';
function feed(h: any, bytes: any, encoding: any) {
  const returned=h.update(bytes,encoding);
  console.log('self',returned===h);
  return h.digest('hex');
}
const source=Buffer.from('prefix-616263-suffix');
const view=source.subarray(7,13);
for(const algorithm of ['sha256','sha384','sha512','sha512-256']){
 const expected=createHash(algorithm).update(view).digest('hex');
 for(const encoding of ['hex','base64','base64url','not-an-encoding']){
  // Local chain and escaped object must both hash the visible bytes literally.
  console.log(algorithm,encoding,createHash(algorithm).update(view,encoding).digest('hex')===expected);
  console.log(feed(createHash(algorithm),view,encoding)===expected);
  const typed=new Uint8Array(source.buffer,source.byteOffset+7,6);
  console.log(feed(createHash(algorithm),typed,encoding)===expected);
  const dataView=new DataView(source.buffer,source.byteOffset+7,6);
  console.log(feed(createHash(algorithm),dataView,encoding)===expected);
  const expectedMac=createHmac(algorithm,'key').update(view).digest('hex');
  console.log(feed(createHmac(algorithm,'key'),view,encoding)===expectedMac);
 }
 console.log('string hex',createHash(algorithm).update('616263','hex').digest('hex')===hash(algorithm,'abc'));
}
const h=createHash('sha512');h.update(view);view.fill(0x78);
console.log('snapshot',h.digest('hex')===hash('sha512','616263'));
