// A list that grew by push past its initial capacity leaves a forwarding stub where the
// head used to be, and a binding that kept the old head (a field no write-back reached,
// the caller's variable after a push through a parameter) reads garbage unless the
// consumer follows the stub. util.format('%j') of an object holding the list.
// One consumer per file: a consumer that fails aborts the run, so a shared file would hide the rest.
import { createRequire } from "node:module";
const req = createRequire(import.meta.url);
const util = req('util');
const path = req('path');
const fs = req('fs');
const work = process.env.WORK_DIR || '/work';

class Holder {
  constructor() {
    this._state = { items: [] };
  }
  add(v) {
    this._state.items.push(v);
  }
  items() {
    return this._state.items;
  }
}
function fill(list, n, mk) {
  for (let i = 0; i < n; i++) list.push(mk(i));
}
// The list as a nested field the push never wrote back, and as the caller's
// binding after a push through a parameter.
function viaField(n, mk) {
  const h = new Holder();
  for (let i = 0; i < n; i++) h.add(mk(i));
  return h.items();
}
function viaParam(n, mk) {
  const list = [];
  fill(list, n, mk);
  return list;
}
// The control: the same elements in a list that never went stale.
function fresh(n, mk) {
  return Array.from(viaParam(n, mk));
}
const shapes = [
  ['fresh', fresh],
  ['field', viaField],
  ['param', viaParam],
];

function run(label, mk, fn) {
  for (const [shape, make] of shapes) {
    const name = label + ' (' + shape + ')';
    try {
      console.log(name, fn(make(40, mk)));
    } catch (e) {
      console.log(name, 'threw', e && e.name, e && e.message);
    }
  }
}
const num = (i) => i * 3 + 1;
const str = (i) => 's' + i;
// Long enough that node prints one element per line: the grouping of a short
// element list into columns is not what this probe is about.
const wide = (i) => 'a-long-element-that-fills-the-line-' + i;
const obj = (i) => ({ a: i, b: 'x' + i });
const pair = (i) => ['k' + i, i];
const code = (i) => 65 + (i % 26);
const byte = (i) => i & 0xff;


run('format %j', num, (l) => util.format('%j', { l }));
