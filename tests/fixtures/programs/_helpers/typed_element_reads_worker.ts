import { parentPort, workerData } from 'node:worker_threads';
const sab=workerData as SharedArrayBuffer;
const control=new Int32Array(sab,0,2);
const vInt8=new Int8Array(sab,16,2);
function readInt8(a: Int8Array): number {let s=0; for(let i=0;i<2;i++) s+=a[i]; return s;}
vInt8[0]=-7; vInt8[1]=19;
const vUint8Clamped=new Uint8ClampedArray(sab,32,2);
function readUint8Clamped(a: Uint8ClampedArray): number {let s=0; for(let i=0;i<2;i++) s+=a[i]; return s;}
vUint8Clamped[0]=-7; vUint8Clamped[1]=19;
const vInt16=new Int16Array(sab,48,2);
function readInt16(a: Int16Array): number {let s=0; for(let i=0;i<2;i++) s+=a[i]; return s;}
vInt16[0]=-7; vInt16[1]=19;
const vUint16=new Uint16Array(sab,64,2);
function readUint16(a: Uint16Array): number {let s=0; for(let i=0;i<2;i++) s+=a[i]; return s;}
vUint16[0]=-7; vUint16[1]=19;
const vInt32=new Int32Array(sab,80,2);
function readInt32(a: Int32Array): number {let s=0; for(let i=0;i<2;i++) s+=a[i]; return s;}
vInt32[0]=-7; vInt32[1]=19;
const vUint32=new Uint32Array(sab,96,2);
function readUint32(a: Uint32Array): number {let s=0; for(let i=0;i<2;i++) s+=a[i]; return s;}
vUint32[0]=-7; vUint32[1]=19;
const vFloat16=new Float16Array(sab,112,2);
function readFloat16(a: Float16Array): number {let s=0; for(let i=0;i<2;i++) s+=a[i]; return s;}
vFloat16[0]=-7; vFloat16[1]=19;
const vFloat32=new Float32Array(sab,128,2);
function readFloat32(a: Float32Array): number {let s=0; for(let i=0;i<2;i++) s+=a[i]; return s;}
vFloat32[0]=-7; vFloat32[1]=19;
const vFloat64=new Float64Array(sab,144,2);
function readFloat64(a: Float64Array): number {let s=0; for(let i=0;i<2;i++) s+=a[i]; return s;}
vFloat64[0]=-7; vFloat64[1]=19;
Atomics.store(control,0,1); Atomics.notify(control,0);
while(Atomics.load(control,1)===0) Atomics.wait(control,1,0,10000);
let sum=0;
sum+=readInt8(vInt8);
sum+=readUint8Clamped(vUint8Clamped);
sum+=readInt16(vInt16);
sum+=readUint16(vUint16);
sum+=readInt32(vInt32);
sum+=readUint32(vUint32);
sum+=readFloat16(vFloat16);
sum+=readFloat32(vFloat32);
sum+=readFloat64(vFloat64);
parentPort!.postMessage(sum);
