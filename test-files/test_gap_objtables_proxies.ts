// Proxy facts follow the receiver cell; targets and handlers may move.
const target: any = { value: 73, gone: 1 };
const handler: any = {};
let observed = 0;
function pressure(): void {
  let last: any;
  for (let i = 0; i < 5000; i++) last = { i, next: { value: i + 1 } };
  observed += last.i;
}
for (const key of ['get', 'set', 'has', 'deleteProperty', 'ownKeys', 'getPrototypeOf', 'isExtensible', 'preventExtensions']) {
  Object.defineProperty(handler, key, { get() { pressure(); return undefined; }, configurable: true });
}
const proxy: any = new Proxy(target, handler);
const nested: any = new Proxy(proxy, {});
const map = new Map<any, number>([[proxy, 9]]);
const set = new Set<any>([nested]);
console.log(nested.value, map.get(proxy), set.has(nested));
proxy.value = 81;
console.log(proxy.value, 'value' in proxy, Reflect.deleteProperty(proxy, 'gone'));
console.log(Reflect.ownKeys(proxy).join(','), Object.getPrototypeOf(proxy) === Object.prototype);
console.log(Reflect.isExtensible(proxy), Reflect.preventExtensions(proxy), Reflect.isExtensible(proxy));
const revocable: any = Proxy.revocable({ x: 5 }, {});
console.log(revocable.proxy.x);
revocable.revoke();
let revoked = false;
try { console.log(revocable.proxy.x); } catch { revoked = true; }
console.log('proxy-owned', revoked, observed > 0, new Set<any>().has(nested));
