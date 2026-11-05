// The native wrapper's per-instance flag survives repeated copying GC.
const native: any = Math.max.bind(null);
console.log(native.length, native(3, 8));
for (let i = 0; i < 8; i++) {
    const garbage: any[] = [];
    for (let j = 0; j < 4000; j++) garbage.push({ j, text: 'flag-' + j });
}
console.log(native.length, native(7, 2));
try { new native(); console.log('constructed'); }
catch (e) { console.log(e instanceof TypeError); }
const ordinary: any = function () { this.x = 11; };
console.log(new ordinary().x);

const proxyCtor = new Proxy(function C(this: any) { this.value = 23; }, {});
console.log((new (proxyCtor as any)()).value);
