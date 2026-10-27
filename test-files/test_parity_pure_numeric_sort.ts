// Admission and exact fallback semantics for pure numeric sorting.
const ints = [2147483647, -4, 2, 2, 0, -2147483648];
console.log(ints.sort((a, b) => a - b).join(','));
console.log(ints.sort((a, b) => b - a).join(','));
console.log([NaN, 3, 1, NaN, 2].sort((a, b) => a - b).join(','));
const zeros = [0, -0, 0, -0, -1, 1].sort((a, b) => a - b);
console.log(zeros.map(x => Object.is(x, -0) ? '-0' : String(x)).join(','));
console.log([Infinity, 2, -Infinity, Infinity, 0.5, 2147483648].sort((a,b) => a-b).join(','));
const records = [{k:2,id:'a'}, {k:1,id:'b'}, {k:2,id:'c'}, {k:1,id:'d'}];
console.log(records.sort((a,b) => a.k-b.k).map(x => x.id).join(','));
const mutate = [3,1,2]; let calls = 0;
mutate.sort((a,b) => { calls++; mutate.push(99); return a-b; });
console.log('mutation', calls > 0, mutate.slice(0,3).join(','), mutate.length > 3);
const throwing = [3,1,2];
try { throwing.sort((a,b) => { throw new Error('cmp'); }); } catch(e) { console.log(e.message, throwing.join(',')); }
const coercions: string[] = [];
const objects = [{valueOf(){coercions.push('x'); return 2;}}, {valueOf(){coercions.push('y'); return 1;}}];
objects.sort((a:any,b:any) => a-b);
console.log('coerce', coercions.length > 0, Number(objects[0]));
const holey = [3, , 1, undefined];
holey.sort((a,b) => a-b);
console.log('holes', holey.join(','), 2 in holey, 3 in holey);
const proto = [3, , 1];
Object.defineProperty(Array.prototype, '1', {value:2,writable:true,configurable:true});
proto.sort((a,b) => a-b);
delete Array.prototype[1];
console.log('inherited', proto.join(','));
const accessed = [3,2,1]; let reads = 0; let writes = 0; let middle = 2;
Object.defineProperty(accessed,'1',{get(){reads++; return middle;},set(v){writes++;middle=v;},configurable:true});
accessed.sort((a,b) => a-b);
console.log('accessor', accessed.join(','), reads > 0, writes > 0);
const proxyEvents: string[] = [];
const proxy = new Proxy([3,2,1], {get(t,k,r){proxyEvents.push('get');return Reflect.get(t,k,r);},set(t,k,v,r){proxyEvents.push('set');return Reflect.set(t,k,v,r);}});
proxy.sort((a,b) => a-b);
console.log('proxy', proxy.join(','), proxyEvents.includes('set'));
const frozen = Object.freeze([3,1,2]);
try { frozen.sort((a,b) => a-b); } catch(e) { console.log('frozen', e instanceof TypeError); }
console.log('sealed', Object.seal([3,1,2]).sort((a,b) => a-b).join(','));
console.log('nonextensible', Object.preventExtensions([3,1,2]).sort((a,b) => a-b).join(','));
const protectedIndex = [3,1,2]; Object.defineProperty(protectedIndex,'0',{writable:false});
try { protectedIndex.sort((a,b) => a-b); } catch(e) { console.log('readonly', e instanceof TypeError); }
const lots: number[] = []; for(let i=2048;i>0;i--) lots.push(i%97);
lots.sort((a,b) => { const garbage = new Array(1024).fill(a); return a-b; });
console.log('gc', lots[0], lots[lots.length-1]);
const typed = new Float64Array([3,1,2]);
console.log('typed', typed.sort((a,b) => a-b).join(','));
const replaced: any = [3,1,2]; replaced.sort = function(cmp) { return ['custom']; };
console.log('override', replaced.sort((a,b) => a-b).join(','));

const parsed: number[] = JSON.parse('[3,1,2]');
console.log('lazy JSON', parsed.sort((a,b) => a-b).join(','));
