function expose(a: Int32Array) {
 for (let i=0;i<3;i++) {
  console.log('expose',a[0]);
  if (i===0) { const alias = new Int32Array(a.buffer); alias[0]=31; }
  if (i===1) a[0] = -19;
 }
}
expose(new Int32Array([7]));
function resize(a: Float64Array, b: ArrayBuffer) {
 for(let i=0;i<4;i++) {
  console.log('resize',i,a[0],a[1],a.length);
  if(i===0) b.resize(8);
  if(i===1) { b.resize(32); a[1]=4.5; }
  if(i===2) b.transfer();
 }
}
const rab = new ArrayBuffer(16,{maxByteLength:32}); const tracking = new Float64Array(rab); tracking[0]=1.5; tracking[1]=2.5; resize(tracking,rab);
const fixedbuf = new ArrayBuffer(24,{maxByteLength:40}); const fixed = new Float64Array(fixedbuf,8,2); fixed[0]=3.5; fixed[1]=4.5; resize(fixed,fixedbuf);
function detachKey(a: Int16Array,b: ArrayBuffer) {
 const key = {toString() { console.log('coercion'); b.transfer(); return '0'; }};
 for(let i=0;i<1;i++) console.log('detached-key',a[key as any]);
}
const db = new ArrayBuffer(4); const da = new Int16Array(db); da[0]=123; detachKey(da,db);
function shared(a: Int32Array) { for(let i=0;i<3;i++) { a[i]=i*100-7; console.log('shared',a[i]); } }
const sb = new SharedArrayBuffer(32); shared(new Int32Array(sb,4,3)); console.log('alias',new Int32Array(sb)[1]);
// Allocate moving holders while entry proof is live.
function moving(a: Float64Array) {
 let sum=0;
 for(let i=0;i<200;i++) { const h={a, values:[i,i+1]}; sum+=a[i&1]+h.values[0]-i; }
 console.log('moving',sum);
}
moving(new Float64Array([0.25,0.75]));
