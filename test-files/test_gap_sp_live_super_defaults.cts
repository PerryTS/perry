class Fixed { constructor(x) { this.x = x; } }
class Rest { constructor(...args) { this.count = args.length; this.args = args.join(','); } }
class D extends Fixed {}
Object.setPrototypeOf(D, Rest);
const d = new D(1, 2, 3);
console.log('rest', d.count, d.args);
class A extends Array {}
Object.setPrototypeOf(A, Array);
console.log('empty-array', new A().length);
console.log('array', Array.prototype.join.call(new A(4, 5), ','));

class Throwing extends Fixed { field = (() => { throw new Error('own field'); })(); }
Object.setPrototypeOf(Throwing, Rest);
try { new Throwing(1, 2); } catch (e) { console.log('initializer-throw', e.message); }

class GenericDefault<T> extends Fixed { marker = 9; }
Object.setPrototypeOf(GenericDefault, Rest);
const genericDefault = new GenericDefault<number>(1, 2, 3);
console.log('generic-default', genericDefault.count, genericDefault.marker);

function derive(Parent, marker) { return class extends Parent { marker = marker; }; }
const DynamicFirst = derive(Fixed, 'one');
const DynamicSecond = derive(Rest, 'two');
const dynamicFirst = new DynamicFirst(8);
console.log('dynamic-first', dynamicFirst.x, dynamicFirst.marker);
console.log('dynamic-second', new DynamicSecond(1, 2, 3).count);
Object.setPrototypeOf(DynamicFirst, Rest);
const dynamicRelinked = new DynamicFirst(1, 2, 3);
console.log('dynamic-relink', dynamicRelinked.count, dynamicRelinked.marker);

class ArgumentDefault extends Fixed {}
function relinkDefault() { Object.setPrototypeOf(ArgumentDefault, Rest); return 1; }
console.log('default-argument-relink', new ArgumentDefault(relinkDefault(), 2, 3).count);

// Exceed the bounded checked-plan walk: use live forwarding, not a partial proof.
class Deep0 { value = 1; }
class Deep1 extends Deep0 {}
class Deep2 extends Deep1 {}
class Deep3 extends Deep2 {}
class Deep4 extends Deep3 {}
class Deep5 extends Deep4 {}
class Deep6 extends Deep5 {}
class Deep7 extends Deep6 {}
class Deep8 extends Deep7 {}
class Deep9 extends Deep8 {}
class Deep10 extends Deep9 {}
class Deep11 extends Deep10 {}
class Deep12 extends Deep11 {}
class Deep13 extends Deep12 {}
class Deep14 extends Deep13 {}
class Deep15 extends Deep14 {}
class Deep16 extends Deep15 {}
class Deep17 extends Deep16 {}
class Deep18 extends Deep17 {}
class Deep19 extends Deep18 {}
class Deep20 extends Deep19 {}
class Deep21 extends Deep20 {}
class Deep22 extends Deep21 {}
class Deep23 extends Deep22 {}
class Deep24 extends Deep23 {}
class Deep25 extends Deep24 {}
class Deep26 extends Deep25 {}
class Deep27 extends Deep26 {}
class Deep28 extends Deep27 {}
class Deep29 extends Deep28 {}
class Deep30 extends Deep29 {}
class Deep31 extends Deep30 {}
class Deep32 extends Deep31 {}
class Deep33 extends Deep32 {}
class Deep34 extends Deep33 {}
class Deep35 extends Deep34 {}
class Deep36 extends Deep35 {}
class Deep37 extends Deep36 {}
class Deep38 extends Deep37 {}
class Deep39 extends Deep38 {}
class Deep40 extends Deep39 {}
class Deep41 extends Deep40 {}
class Deep42 extends Deep41 {}
class Deep43 extends Deep42 {}
class Deep44 extends Deep43 {}
class Deep45 extends Deep44 {}
class Deep46 extends Deep45 {}
class Deep47 extends Deep46 {}
class Deep48 extends Deep47 {}
class Deep49 extends Deep48 {}
class Deep50 extends Deep49 {}
class Deep51 extends Deep50 {}
class Deep52 extends Deep51 {}
class Deep53 extends Deep52 {}
class Deep54 extends Deep53 {}
class Deep55 extends Deep54 {}
class Deep56 extends Deep55 {}
class Deep57 extends Deep56 {}
class Deep58 extends Deep57 {}
class Deep59 extends Deep58 {}
class Deep60 extends Deep59 {}
class Deep61 extends Deep60 {}
class Deep62 extends Deep61 {}
class Deep63 extends Deep62 {}
class Deep64 extends Deep63 {}
class Deep65 extends Deep64 {}
class Deep66 extends Deep65 {}
class DeepAlternate { value = 50; }
Object.setPrototypeOf(Deep1, DeepAlternate);
console.log("deep-default", new Deep66().value);

class DeclaredParent {}
class ActualParent { constructor() { this.parentSaw = Object.keys(this).join(','); this.dynamic = 1; } }
class OrderedDefault extends DeclaredParent { own = 2; }
Object.setPrototypeOf(OrderedDefault, ActualParent);
const orderedDefault = new OrderedDefault();
console.log('parent-saw', orderedDefault.parentSaw);
console.log('default-order', Object.keys(orderedDefault).join(','));
