function read(a: any[], k: any) { return a[k]; }
function len(a: any[]) { return a.length; }
// Object.prototype alone must invalidate the empty-chain proof, before any
// Array.prototype indexed mutation has happened in this process.
if (process.argv[2] === 'object-first') {
const objectFirst: any[] = [14, , ];
Object.defineProperty(Object.prototype, '23', {configurable:true,get(){return this[0]+50;}});
console.log('object first oob',read(objectFirst,23));
objectFirst.length=24;console.log('object first hole',read(objectFirst,23));
delete Object.prototype[23];
}
const a: any[] = [11, , 33];
a[-1] = 'negative'; a[1.5] = 'fraction'; a['NaN'] = 'nan'; a['01'] = 'leading';
console.log(read(a,0), read(a,1), read(a,2), read(a,3), read(a,999999));
console.log(read(a,-1), read(a,1.5), read(a,NaN), read(a,'0'), read(a,'01'), read(a,'length'));
console.log(read(a,-0), read(a,Infinity), read(a,2147483648), read(a,4294967295));
let getterCalls=0;
Object.defineProperty(Array.prototype, '20', {configurable:true, get(){getterCalls++; return 71;}});
const h: any[] = new Array(24); h[0]=9;
console.log(read(h,20), read(h,0), read(h,21), getterCalls);
delete Array.prototype[20];
Object.defineProperty(h, '2', {configurable:true, get(){return 72;}});
console.log(read(h,2),len(h));
const p: any[] = [1,,3];
Object.setPrototypeOf(p, {1:81, length:99});
console.log(read(p,0),read(p,1),read(p,2),len(p));
const f: any[] = Object.freeze([4,5,6]);
console.log(read(f,1),len(f));
const xs: any[] = [1,2,3,4]; let total=0;
function shrink(){ xs.length=2; }
for(let i=0;i<xs.length;i++){total+=xs[i]; if(i===0)shrink();}
console.log(total,len(xs));
xs.length=5;console.log(len(xs),read(xs,3));xs[4]=8;console.log(read(xs,4));xs.length=1;console.log(len(xs),read(xs,4));
let grow: any[]=[1,2];let gsum=0;
for(let i=0;i<grow.length;i++){gsum+=grow[i];if(i===0)grow.push(3);}
console.log(gsum,len(grow));
const alias: any[]=[1,2,3,4]; const old=alias;
for(let i=0;i<100;i++)alias.push(i);
console.log(read(old,3),read(old,99),len(old));
function countCaptured(){const cap=[1,2,3];return ()=>{let s=0;for(let i=0;i<cap.length;i++)s+=cap[i];return s;};}
console.log(countCaptured()());
const fake: any = {0:'zero',length:'custom'};
console.log(read(fake,0),len(fake));
const proxy: any = new Proxy([7,8],{get(t,k){return k==='0'?91:Reflect.get(t,k);}});
console.log(read(proxy,0),len(proxy));
console.log(read('abc' as any,'1'),len('abc' as any));
// The checked backend must preserve shifted/named storage and typed arrays.
const shifted: any[]=[3,4,5];shifted.shift();shifted.extra=6;
console.log(read(shifted,0),read(shifted,1),len(shifted),shifted.extra);
const typed: any=new Uint8Array([9,10]);
console.log(read(typed,0),read(typed,1),read(typed,2),len(typed));
// A receiver whose shape is unrelated to Array is the negative control.
console.log(read({0:123,length:2} as any,0),len({length:2} as any));

// Both generic-key and numeric consumers share the same linked cold body.
function numericRead(a: number[], k: number) { return a[k] * 2; }
const nums: number[] = [2, 4, 6];
console.log(numericRead(nums, 1), numericRead(nums, -1), numericRead(nums, 20), numericRead(nums, 0.5), numericRead(nums, NaN));
Object.defineProperty(Array.prototype, '20', {configurable:true, get(){return 17;}});
const sparseNumbers: number[] = new Array(24);
console.log(numericRead(sparseNumbers, 20), numericRead(sparseNumbers, 21));
delete Array.prototype[20];
const numericAlias = nums; for(let i=0;i<100;i++) nums.push(i);
console.log(numericRead(numericAlias, 99));
// Shared hole/OOB path preserves accessor receiver and precedence at both
// intrinsic prototypes, then walks a replaced chain (including proxies).
const inherited: any[] = [17, , ];
(inherited as any).marker = 80;
Object.defineProperty(Array.prototype, '23', {configurable:true, get(){return this[0] + this.marker;}});
Object.defineProperty(Object.prototype, '24', {configurable:true, get(){return this[0] + this.marker + 1;}});
inherited.length = 25;
console.log('inherited holes', read(inherited,23), read(inherited,24));
inherited.length = 1;
console.log('inherited oob', read(inherited,23), read(inherited,24), read(inherited,25));
Object.defineProperty(Object.prototype, '23', {configurable:true, get(){return 900;}});
console.log('precedence', read(inherited,23));
delete Array.prototype[23]; delete Object.prototype[23]; delete Object.prototype[24];
const chain: any = Object.create(Array.prototype);
Object.defineProperty(chain, '23', {configurable:true, get(){return this[0] + this.marker + 2;}});
Object.setPrototypeOf(inherited,chain);
inherited.length=25; console.log('custom holes',read(inherited,23),read(inherited,24));
inherited.length=1; console.log('custom oob',read(inherited,23),read(inherited,24));
Object.setPrototypeOf(inherited,null); console.log('null proto',read(inherited,23));
const proxyProto: any = new Proxy({23:100}, {get(t,k,r){return k==='23' ? r[0]+t[k] : Reflect.get(t,k,r);}});
Object.setPrototypeOf(inherited,proxyProto); console.log('proxy proto',read(inherited,23));
const arrayProto: any[] = [];
Object.defineProperty(arrayProto,'23',{configurable:true,get(){return this[0]+this.marker+3;}});
Object.setPrototypeOf(inherited,arrayProto); console.log('array proto',read(inherited,23));
// Restore the known intrinsic chain directly: getPrototypeOf is a separate API.
Object.setPrototypeOf(Array.prototype,{26:123});
console.log('intrinsic relink',read([1],26));
Object.setPrototypeOf(Array.prototype,Object.prototype);
Array.prototype[27]=127; Object.prototype[28]=128;
const protoData: any[] = new Array(30);
console.log('prototype data holes',read(protoData,27),read(protoData,28));
protoData.length=0;
console.log('prototype data oob',read(protoData,27),read(protoData,28));
Object.prototype[29]=129;
Object.defineProperty(Array.prototype,'29',{configurable:true,get(){return undefined;}});
console.log('undefined getter shadows',read(protoData,29));
delete Array.prototype[27]; delete Object.prototype[28]; delete Array.prototype[29]; delete Object.prototype[29];

const originalObject = Object;
Object.prototype[30] = 130;
globalThis.Object = {prototype:{30:999}} as any;
console.log('intrinsic object after global replace',read(protoData,30));
globalThis.Object = originalObject;
delete Object.prototype[30];

// Array algorithms use the same rooted own/prototype walk as a cold GET.
const ownWalk: any[] = [11,,33];
Object.defineProperty(ownWalk,'1',{configurable:true,get(){return this[0]+10;}});
console.log('own algorithm getter',ownWalk.slice(1,2)[0],ownWalk.slice(0,3).join(','));
const farOwn: any[] = [1]; farOwn[2000000]=41;
console.log('own sparse algorithm',farOwn.slice(2000000,2000001)[0],farOwn.slice(1999999,2000000)[0]);
console.log('own shifted algorithm',shifted.slice(0,2).join(','));
Object.defineProperty(ownWalk,'1',{configurable:true,get:undefined,set(v){}});
console.log('own setter shadows',read(ownWalk,1),ownWalk.slice(1,2)[0]);
const frozenProto: any[] = Object.freeze([4,45]);
const frozenChild: any[] = new Array(2); Object.setPrototypeOf(frozenChild,frozenProto);
console.log('frozen prototype data',read(frozenChild,1));
