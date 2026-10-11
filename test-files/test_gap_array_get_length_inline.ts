function read(a: any[], k: any) { return a[k]; }
function len(a: any[]) { return a.length; }
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
