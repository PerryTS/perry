// #11743: field-array append retains Reference/Get/argument/Call order.
class Holder {
  children: number[] = [];
}
function allocationNoise(): number {
  const scratch: Holder[] = [];
  for (let i = 0; i < 1000; i++) scratch.push(new Holder());
  return scratch.length;
}
function append(h: Holder, value: () => number): void {
  h.children.push(value());
}
const h = new Holder();
const original = h.children;
append(h, () => { h.children = [90]; return 7; });
console.log('receiver', original.join(','), h.children.join(','));

let log = '';
const observed = new Holder();
const backing: number[] = [];
Object.defineProperty(observed, 'children', {
  get() { log += 'field;'; return backing; },
});
append(observed, () => { log += 'arg;'; return 8; });
console.log('getter', log, backing.join(','));

const overridden = new Holder();
(overridden.children as any).push = function(v: number) {
  log += 'old:' + v + ':' + (this === overridden.children) + ';';
  return 123;
};
log = '';
append(overridden, () => {
  log += 'arg;';
  allocationNoise();
  (overridden.children as any).push = () => { log += 'new;'; return 456; };
  return 9;
});
console.log('own snapshot', log, overridden.children.length);

const installed = new Holder();
log = '';
append(installed, () => {
  (installed.children as any).push = () => { log += 'installed;'; return 1; };
  allocationNoise();
  return 10;
});
console.log('builtin snapshot', log, installed.children.join(','));

const methodGetter = new Holder();
log = '';
Object.defineProperty(methodGetter.children, 'push', {
  get() {
    log += 'method;';
    return function(v: number) { log += 'call:' + v + ';'; return 20; };
  },
});
append(methodGetter, () => { log += 'arg;'; return 11; });
console.log('method getter', log);

class CustomArray extends Array<number> {
  push(v: number): number { log += 'sub:' + v + ';'; return 30; }
}
const sub = new Holder();
sub.children = new CustomArray();
log = '';
append(sub, () => 12);
console.log('subclass', log, sub.children.length);

const wrong = new Holder();
wrong.children = { push(v: number) { log += 'object:' + v + ';'; } } as any;
log = '';
append(wrong, () => 13);
console.log('wrong annotation', log);

const frozen = new Holder();
Object.freeze(frozen.children);
try { append(frozen, () => 14); } catch (e) { console.log('frozen', e instanceof TypeError); }

// An older field array receives recursively allocating arguments. Parent
// identity is observed after churn, including arrays that grew beyond capacity.
class DocNode {
  children: DocNode[] = [];
  parent: DocNode | null;
  constructor(parent: DocNode | null) { this.parent = parent; }
}
function tree(depth: number, parent: DocNode | null): DocNode {
  const node = new DocNode(parent);
  if (depth > 0) {
    for (let i = 0; i < 7; i++) node.children.push(tree(depth - 1, node));
  }
  return node;
}
function count(n: DocNode): number {
  let result = 1;
  for (let i = 0; i < n.children.length; i++) {
    const c = n.children[i];
    if (c.parent !== n) throw new Error('parent identity');
    result += count(c);
  }
  return result;
}
let checksum = 0;
const retained: DocNode[] = [];
for (let i = 0; i < 80; i++) {
  const root = tree(3, null);
  checksum += count(root);
  if (i % 10 === 0) retained.push(root);
}
for (let i = 0; i < retained.length; i++) checksum += count(retained[i]);
console.log('recursive', checksum);
