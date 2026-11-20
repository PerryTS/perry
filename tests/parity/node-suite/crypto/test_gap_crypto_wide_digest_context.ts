import { createHash, createHmac } from 'node:crypto';
for (const algorithm of ['sha384','sha512']) {
 for (const size of [0,1,127,128,129,255,256,257,4096]) {
  const source=Buffer.alloc(size+6,0x61);
  const data=source.subarray(3,3+size);
  const at=Math.min(127,size);
  const hash=createHash(algorithm);
  hash.update(data.subarray(0,at));
  const copy=hash.copy();
  hash.update(data.subarray(at));copy.update(data.subarray(at));
  console.log(algorithm,size,hash.digest('hex'),copy.digest('base64'));
  const mac=createHmac(algorithm,Buffer.alloc(257,0x78));
  mac.update(data.subarray(0,at));mac.update(data.subarray(at));
  console.log('mac',mac.digest('hex'),'second',mac.digest('hex'));
 }
 const done=createHash(algorithm);done.digest();
 try{done.update(Buffer.from('x'));}catch(error:any){console.log('finalized',error.code);}
}
