// One read site sees more receiver shapes than the 16 class ways.
// After the cache is warm, mutate receivers, intermediate prototypes and
// the terminal. Each round visits every shape again; getters count calls.
class Root { kind = 0; }
class L1 extends Root {}
class L2 extends L1 {}
class L3 extends L2 {}
class L4 extends L3 {}
class L5 extends L4 {}
class L6 extends L5 {}
class L7 extends L6 {}
class Leaf extends L7 {}
const nodes: any[] = [];
for (let i = 0; i < 96; i++) {
  const o: any = new Leaf();
  o.kind = i;
  o["shape" + i] = i;
  nodes.push(o);
}
function read(o: any): any { return o.comments; }
function warm(reader: (o: any) => any = read): void {
  for (let r = 0; r < 20; r++) for (const o of nodes) reader(o);
}
function show(label: string, reader: (o: any) => any = read): void {
  const values: string[] = [];
  for (const o of nodes) values.push(String(reader(o)));
  console.log(label, values.join(","));
}
warm();
show("absent-96");
(Root.prototype as any).comments = "root";
show("prototype-added");
delete (Root.prototype as any).comments;
warm();
nodes[37].comments = "own37";
show("instance-added");
delete nodes[37].comments;
warm();
(L4.prototype as any).comments = "middle";
show("middle-added");
delete (L4.prototype as any).comments;
warm();
// Keep the terminal/receiver/relink checks ahead of the prototype getter,
// which correctly latches class priming at this site after it is observed.
(Object.prototype as any).comments = "terminal";
show("terminal-added");
delete (Object.prototype as any).comments;
warm();
const alternate: any = { comments: "relinked" };
Object.setPrototypeOf(L3.prototype, alternate);
show("middle-relinked");
Object.setPrototypeOf(L3.prototype, L2.prototype);
warm();
Object.setPrototypeOf(nodes[18], { comments: "instance-proto" });
show("instance-relinked");
Object.setPrototypeOf(nodes[18], Leaf.prototype);
warm();
Object.defineProperty(nodes[45], "comments", {
  configurable: true,
  get() { return "own-getter45"; },
});
show("instance-getter");
delete nodes[45].comments;
warm();
function readGetter(o: any): any { return o.comments; }
warm(readGetter);
let calls = 0;
Object.defineProperty(L6.prototype, "comments", {
  configurable: true,
  get() { calls++; return "getter" + this.kind; },
});
show("middle-getter", readGetter);
console.log("getter-calls", calls);
delete (L6.prototype as any).comments;
// A distinct key and class family keep this site's absent proof live
// independently of the accessor refusal above.
class NullRoot { kind = 0; }
class NullMid extends NullRoot {}
class NullLeaf extends NullMid {}
const nullClassNodes: any[] = [];
for (let i = 0; i < 96; i++) {
  const o: any = new NullLeaf();
  o.kind = i;
  o["nullShape" + i] = i;
  nullClassNodes.push(o);
}
function readNullRoot(o: any): any { return o.prettierIgnore; }
function warmNullRoot(): void {
  for (let r = 0; r < 20; r++) for (const o of nullClassNodes) readNullRoot(o);
}
function showNullRoot(label: string): void {
  console.log(label, nullClassNodes.map(readNullRoot).map(String).join(","));
}
Object.setPrototypeOf(NullRoot.prototype, null);
warmNullRoot();
showNullRoot("null-class-root");
(NullRoot.prototype as any).prettierIgnore = "null-root-added";
showNullRoot("null-root-added");
delete (NullRoot.prototype as any).prettierIgnore;
warmNullRoot();
Object.setPrototypeOf(NullRoot.prototype, { prettierIgnore: "null-root-relinked" });
showNullRoot("null-root-relinked");
// Pure null-prototype receivers share the read site with class receivers.
function readNull(o: any): any { return o.extra; }
const nulls: any[] = [];
for (let i = 0; i < 32; i++) {
  const o: any = Object.create(null);
  o["n" + i] = i;
  nulls.push(o);
}
for (let r = 0; r < 20; r++) for (const o of nulls) readNull(o);
console.log("null-absent", nulls.map(readNull).join(","));
nulls[5].extra = "null-own5";
Object.setPrototypeOf(nulls[8], { extra: "null-proto8" });
console.log("null-changed", nulls.map(readNull).join(","));

