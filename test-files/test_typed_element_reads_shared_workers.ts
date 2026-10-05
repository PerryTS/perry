import { Worker } from 'node:worker_threads';
const sab = new SharedArrayBuffer(256);
const control=new Int32Array(sab,0,2);
const vInt8=new Int8Array(sab,16,2);
function readInt8(a: Int8Array): number {let s=0; for(let i=0;i<2;i++) s+=a[i]; return s;}
const vUint8Clamped=new Uint8ClampedArray(sab,32,2);
function readUint8Clamped(a: Uint8ClampedArray): number {let s=0; for(let i=0;i<2;i++) s+=a[i]; return s;}
const vInt16=new Int16Array(sab,48,2);
function readInt16(a: Int16Array): number {let s=0; for(let i=0;i<2;i++) s+=a[i]; return s;}
const vUint16=new Uint16Array(sab,64,2);
function readUint16(a: Uint16Array): number {let s=0; for(let i=0;i<2;i++) s+=a[i]; return s;}
const vInt32=new Int32Array(sab,80,2);
function readInt32(a: Int32Array): number {let s=0; for(let i=0;i<2;i++) s+=a[i]; return s;}
const vUint32=new Uint32Array(sab,96,2);
function readUint32(a: Uint32Array): number {let s=0; for(let i=0;i<2;i++) s+=a[i]; return s;}
const vFloat16=new Float16Array(sab,112,2);
function readFloat16(a: Float16Array): number {let s=0; for(let i=0;i<2;i++) s+=a[i]; return s;}
const vFloat32=new Float32Array(sab,128,2);
function readFloat32(a: Float32Array): number {let s=0; for(let i=0;i<2;i++) s+=a[i]; return s;}
const vFloat64=new Float64Array(sab,144,2);
function readFloat64(a: Float64Array): number {let s=0; for(let i=0;i<2;i++) s+=a[i]; return s;}
const worker=new Worker(new URL('./_helpers/typed_element_reads_worker.ts',import.meta.url),{workerData:sab});
while(Atomics.load(control,0)===0) Atomics.wait(control,0,0,10000);
console.log('Int8',readInt8(vInt8)); vInt8[0]=23; vInt8[1]=29;
console.log('Uint8Clamped',readUint8Clamped(vUint8Clamped)); vUint8Clamped[0]=23; vUint8Clamped[1]=29;
console.log('Int16',readInt16(vInt16)); vInt16[0]=23; vInt16[1]=29;
console.log('Uint16',readUint16(vUint16)); vUint16[0]=23; vUint16[1]=29;
console.log('Int32',readInt32(vInt32)); vInt32[0]=23; vInt32[1]=29;
console.log('Uint32',readUint32(vUint32)); vUint32[0]=23; vUint32[1]=29;
console.log('Float16',readFloat16(vFloat16)); vFloat16[0]=23; vFloat16[1]=29;
console.log('Float32',readFloat32(vFloat32)); vFloat32[0]=23; vFloat32[1]=29;
console.log('Float64',readFloat64(vFloat64)); vFloat64[0]=23; vFloat64[1]=29;
Atomics.store(control,1,1); Atomics.notify(control,1);
await new Promise<void>((resolve,reject)=>{worker.on('message',(sum)=>{console.log('worker sum',sum);resolve();});worker.on('error',reject);});
