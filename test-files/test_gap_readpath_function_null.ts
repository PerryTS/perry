function F(this: any) { this.own = 7; }
(F as any).prototype.k = 'old';
(F as any).prototype.m = function () { return 'method'; };
function named(o: any) { return o.k; }
function keyed(o: any, k: string) { return o[k]; }
function call(o: any) { return o.m(); }
const direct: any = new (F as any)();
const hop: any = new (F as any)();
const child: any = Object.create(hop);
for (let i = 0; i < 100; i++) { named(direct); keyed(child, 'k'); call(child); }
console.log('before', named(direct), keyed(child, 'k'), call(child));
Object.setPrototypeOf(direct, null);
Object.setPrototypeOf(hop, null);
for (const o of [direct, child]) {
    console.log('null', named(o), keyed(o, 'k'), typeof o.m, 'k' in o, o.own);
    try { console.log('call', call(o)); }
    catch (e: any) { console.log('call', e instanceof TypeError); }
}
(F as any).prototype.k = 'later';
console.log('still null', named(direct), keyed(child, 'k'));
Object.setPrototypeOf(hop, { k: 'new', m() { return 'new method'; } });
console.log('relinked', named(child), keyed(child, 'k'), call(child));
