// Store-plan cache invalidation semantics — must print identically under node and perry.
var out = [];

// 1) Per-instance setPrototypeOf AFTER the (class,key) plan is warm:
//    the overridden instance must dispatch the inherited setter, not store a fast own slot.
function F() { this.x = 1; }
var o1 = new F(), o2 = new F();
for (var i = 0; i < 50000; i++) { o1.y = i; }  // warm (F,"y") as a fast plain store
var log1 = [];
Object.setPrototypeOf(o2, { set y(v) { log1.push("setter:" + v); }, get y() { return "got"; } });
o2.y = 99;
out.push("t1=" + log1.join(",") + "|" + o2.y + "|" + o1.y);

// 2) Prototype-level accessor installed AFTER warm-up: fresh instances must dispatch it.
function G() { this.a = 0; }
var g1 = new G(), g2 = new G();
for (var i = 0; i < 50000; i++) { g1.w = i; }  // warm (G,"w")
var log2 = [];
Object.defineProperty(Object.getPrototypeOf(g2), "w", {
  set: function (v) { log2.push("ps:" + v); },
  get: function () { return "pg"; }
});
var g3 = new G();
g3.w = 7;  // no own "w" — must route to the prototype setter
out.push("t2=" + log2.join(",") + "|" + g3.w + "|" + g1.w);

// 3) Freeze AFTER warm-up: writes must be dropped (sloppy mode) on the frozen instance only.
function H() { this.q = 1; }
var h1 = new H(), h2 = new H();
for (var i = 0; i < 50000; i++) { h1.q = i; }
Object.freeze(h2);
h2.q = 123;
out.push("t3=" + h2.q + "|" + h1.q);

// 4) Non-writable data property on the prototype after warm-up blocks fresh instances' stores.
function K() {}
var k1 = new K();
for (var i = 0; i < 50000; i++) { k1.m = i; }
Object.defineProperty(Object.getPrototypeOf(k1), "n", { value: 42, writable: false });
var k2 = new K();
k2.n = 7;  // inherited non-writable — sloppy-mode silent no-op, no own prop
out.push("t4=" + k2.n + "|" + Object.prototype.hasOwnProperty.call(k2, "n"));

console.log(out.join(" "));
