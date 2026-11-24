// parity-node-argv: --experimental-webstorage --localstorage-file=/tmp/perry_web_storage_proto_store.sqlite

// A program store onto Storage.prototype replaces the member a storage
// object's method call runs; the call reaches the program's function with
// the storage object as this. A program write of a member name on the
// storage object itself is the named setter (a stored item), never a method.
localStorage.clear();
const box = { hits: 0, k: "", v: "", self: false };
const orig = Storage.prototype.setItem;
const mine = function (this: any, k: string, v: string) {
  box.hits++;
  box.k = k;
  box.v = v;
  box.self = this === localStorage;
};
(Storage.prototype as any).setItem = mine;
console.log(localStorage.setItem === mine, Object.getPrototypeOf(localStorage) === Storage.prototype);
localStorage.setItem("a", "b");
console.log(box.hits, box.k, box.v, box.self, localStorage.getItem("a"));
(Storage.prototype as any).setItem = orig;
localStorage.setItem("c", "d");
console.log(box.hits, localStorage.getItem("c"), localStorage.length);
(Storage.prototype as any).getItem = function (this: any, k: string) { return "proto:" + k; };
console.log(localStorage.getItem("c"), sessionStorage.getItem("zz"));
try {
  (localStorage as any).nope();
  console.log("no throw");
} catch (e: any) {
  console.log("threw", e instanceof TypeError);
}
localStorage.clear();
