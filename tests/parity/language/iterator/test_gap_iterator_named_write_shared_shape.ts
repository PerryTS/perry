// #9019: a named write on a built-in iterator lands past its raw fields, also
// when an ordinary object of the same prototype (Object.create of the
// iterator prototype) taught the same store site its first-key edge.
const proto: any = Object.getPrototypeOf([1].values());
const sproto: any = Object.getPrototypeOf(new Set([1]).values());
for (let i = 0; i < 3; i++) { const o: any = Object.create(proto); o.foo = i; const p: any = Object.create(sproto); p.foo = i; }
function put(t: any, v: number) { t.foo = v; }
for (let i = 0; i < 3; i++) put(Object.create(proto), i);
const it: any = [5, 6, 7].values();
put(it, 1);
const st: any = new Set([8, 9]).values();
put(st, 2);
console.log(JSON.stringify(it.next()), it.foo, JSON.stringify(st.next()), st.foo, [...it].join(), [...st].join());
