// #10477: `x instanceof F` was always false when `F` is an IMPORTED non-class
// constructor function, for every import form. HIR only routed a bare
// identifier RHS through the dynamic `js_instanceof_dynamic` path when it was a
// local / module function / native module; an imported binding fell to the
// static class-id path, which has no id for a function and folded to `false`.
// `ns.F`, a local alias, and the check inside the defining module all worked.
// Imported classes are controls: they keep the static class-id check.

import {
  Plain,
  Swapped,
  Klass,
  SubKlass,
  ExprKlass,
  Base,
  Inherited,
  Linked,
  Duck,
  Headers,
  EventEmitter,
  Stream,
  MadeConst,
  Rebound,
  rebind,
  notCallable,
  makePlain,
  makeSwapped,
  isPlainHere,
} from "./fixtures/issue_10477_fn_ctor/lib.ts";
import { Plain as RenamedPlain, Klass as RenamedKlass } from "./fixtures/issue_10477_fn_ctor/lib.ts";
import * as ns from "./fixtures/issue_10477_fn_ctor/lib.ts";
import DefaultFn from "./fixtures/issue_10477_fn_ctor/default_fn.ts";
import D from "./fixtures/issue_10477_fn_ctor/default_var.ts";
import CjsCtor from "./fixtures/issue_10477_fn_ctor/cjs_default.cjs";
import { Named } from "./fixtures/issue_10477_fn_ctor/cjs_named.cjs";

const show = (label: string, value: unknown) => console.log(label, value);
const P: any = Plain;
const S: any = Swapped;

// Prototype untouched.
show("plain importer-made", new P(1) instanceof Plain);
show("plain definer-made", makePlain(1) instanceof Plain);
show("plain renamed import", new P(1) instanceof RenamedPlain);
show("plain in defining module", isPlainHere(new P(1)));
show("plain via namespace", new P(1) instanceof ns.Plain);
const Alias = Plain;
show("plain via local alias", new P(1) instanceof Alias);
show("plain Object.create", Object.create(P.prototype) instanceof Plain);
show("plain method", new P(7).get());
show("plain vs {}", ({}) instanceof Plain);
show("plain vs number", (1 as any) instanceof Plain);
show("plain vs null", (null as any) instanceof Plain);
show("plain vs Swapped", new P(1) instanceof Swapped);

// Prototype replaced.
show("swapped importer-made", new S(1) instanceof Swapped);
show("swapped definer-made", makeSwapped(1) instanceof Swapped);
show("swapped Object.create", Object.create(S.prototype) instanceof Swapped);
show("swapped vs Plain", new S(1) instanceof Plain);

// Class controls.
show("klass", new Klass(1) instanceof Klass);
show("klass renamed import", new Klass(1) instanceof RenamedKlass);
show("subklass is klass", new SubKlass(1) instanceof Klass);
show("klass vs subklass", new Klass(1) instanceof SubKlass);
show("class expression", new ExprKlass() instanceof ExprKlass);
show("plain vs klass", new P(1) instanceof Klass);
show("klass vs plain", new Klass(1) instanceof Plain);

// ES5 inheritance.
const I: any = Inherited;
const L: any = Linked;
show("util.inherits child", new I() instanceof Inherited);
show("util.inherits base", new I() instanceof Base);
show("setPrototypeOf child", new L() instanceof Linked);
show("setPrototypeOf base", new L() instanceof Base);
show("base vs child", new (Base as any)() instanceof Inherited);

// Symbol.hasInstance override.
show("hasInstance yes", ({ quack: true }) instanceof Duck);
show("hasInstance no", ({ quack: false }) instanceof Duck);

// Names that collide with a builtin the static path maps to a reserved class
// id. (The instances are built through a local alias: `new (Headers as any)()`
// still routes to the BUILTIN constructor, which is a separate defect.)
const UserHeaders: any = Headers;
const UserEmitter: any = EventEmitter;
const UserStream: any = Stream;
show("user Headers", new UserHeaders() instanceof Headers);
show("user EventEmitter", new UserEmitter() instanceof EventEmitter);
show("user Stream", new UserStream() instanceof Stream);
show("{} vs user Headers", ({}) instanceof Headers);

// Other import forms.
show("factory const", MadeConst(1) instanceof MadeConst);
show("factory const new", new MadeConst(2) instanceof MadeConst);
show("export default function", new (DefaultFn as any)(1) instanceof DefaultFn);
show("export default var", D(1) instanceof D);
show("export default var new", new D(2) instanceof D);
show("cjs module.exports", new (CjsCtor as any)(1) instanceof CjsCtor);
show("cjs exports.Named", new (Named as any)(1) instanceof Named);
show("cjs vs Plain", new (CjsCtor as any)(1) instanceof Plain);

// Live binding: the check reads the binding's current value.
const first = new Rebound();
show("rebound before", first instanceof Rebound);
rebind();
show("rebound after", first instanceof Rebound);
show("rebound new", new Rebound() instanceof Rebound);

// A non-callable import is a TypeError, not a silent false.
try {
  show("non-callable", ({}) instanceof notCallable);
} catch (e) {
  show("non-callable throws", (e as Error).constructor.name);
}
