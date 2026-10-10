// Two receivers, absent/inherited leaves, defaults, and source-ordered misses.
function probe(a: any, b: any): string {
  const ax: any = a.x;
  const ae: any = a.x - 1 + (a.rowSpan || 1);
  const bx: any = b.x;
  const be: any = b.x - 1 + (b.rowSpan || 1);
  const eq: any = a.colSpan === b.colSpan;
  return [ax, ae, bx, be, eq].map(v => String(v)).join(',');
}
const fixed: any = { x: 4, rowSpan: 2 };
function warm(a: any) { let s = ''; for (let i=0;i<12;i++) s = probe(a, fixed); console.log(s); }
const own: any = { x: 2 }; warm(own); own.rowSpan=3; warm(own);
const proto: any = { x: 2 }; const child: any = Object.create(proto); warm(child); proto.rowSpan=4; warm(child);
const deep: any = Object.create(Object.create(proto)); warm(deep); proto.x=6; warm(deep);
// Deeper than the existing publisher admission limit: exact generic miss.
let veryDeep: any = proto; for (let i=0;i<8;i++) veryDeep=Object.create(veryDeep); warm(veryDeep);
const events: string[]=[];
Object.defineProperty(proto, 'rowSpan', { configurable:true, get() { events.push('a'); fixed.x++; return 5; } });
console.log(probe(child, fixed), events.join(''));
delete proto.rowSpan;
Object.setPrototypeOf(child, {x:8,rowSpan:3}); warm(child);
const polluted: any = {x:3}; warm(polluted);
(Object.prototype as any).rowSpan=7; warm(polluted); delete (Object.prototype as any).rowSpan; warm(polluted);
const nil: any = Object.create(null); nil.x=2; warm(nil); nil.rowSpan=0; warm(nil);
const missing: any = Object.create(null); warm(missing);
function undefinedMath(a:any,b:any) {
 const a0:any=a.x - 1 + (a.rowSpan || 1);
 const b0:any=b.x - 1 + (b.rowSpan || 1);
 const a1:any=+a.x;
 const b1:any=-b.x;
 const a2:any=a.x && 1;
 const b2:any=b.x || 1;
 console.log(Number.isNaN(a0),Number.isNaN(b0),Number.isNaN(a1),Number.isNaN(b1),String(a2),b2);
}
undefinedMath(missing,missing);
// Non-numeric data is the negative semantic control; coercion must run in order.
const coercion:any={x:{valueOf(){events.push('coerce');return 9;}}};
console.log(probe(coercion,fixed),events.join(','));
