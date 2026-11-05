class Base { constructor(v) { this.base = v; } }
class Alternate { constructor(v) { this.alternate = v + 50; this.target = new.target.name; } }
class Leaf extends Base { constructor(v) { super(v); this.leaf = 4; } }
const first = new Leaf(1);
console.log('first', first.base, first.leaf);
Object.setPrototypeOf(Leaf, Alternate);
const second = new Leaf(2);
console.log('second', second.base, second.alternate, second.leaf, second.target);
Object.setPrototypeOf(Leaf, Base);
console.log('restore', new Leaf(3).base);
class Before extends Base { constructor(v) { super(v); } }
Before.__proto__ = Alternate;
console.log('before', new Before(4).alternate);
class Implicit extends Base { own = 9; }
Object.setPrototypeOf(Implicit, Alternate);
const implicit = new Implicit(5);
console.log('implicit', implicit.base, implicit.alternate, implicit.own, implicit.target);
class Spread extends Base { constructor(...args) { super(...args); this.own = 7; } }
Object.setPrototypeOf(Spread, Alternate);
const spread = new Spread(6);
console.log('spread', spread.base, spread.alternate, spread.own, spread.target);
class Arrow extends Base { constructor(v) { const run = () => super(v); run(); this.own = 8; } }
Object.setPrototypeOf(Arrow, Alternate);
const arrow = new Arrow(7);
console.log('arrow', arrow.base, arrow.alternate, arrow.own);
class Target {}
class ReflectLeaf extends Base { constructor(v) { super(v); this.leaf = 4; } }
Object.setPrototypeOf(ReflectLeaf, Alternate);
const reflected = Reflect.construct(ReflectLeaf, [8], Target);
console.log('reflect', reflected.alternate, reflected.target, reflected.leaf, Object.getPrototypeOf(reflected) === Target.prototype);
class Captured extends Base { constructor() { super((Object.setPrototypeOf(Captured, Alternate), 9)); } }
console.log('args-first', new Captured().base, new Captured().alternate);
class SpreadCaptured extends Base { constructor(...args) { super(...args); } }
const iterator = { [Symbol.iterator]() { Object.setPrototypeOf(SpreadCaptured, Alternate); return [10][Symbol.iterator](); } };
console.log('spread-first', new SpreadCaptured(...iterator).alternate);
Object.setPrototypeOf(Leaf, null);
try { new Leaf(0); } catch (e) { console.log('null', e instanceof TypeError); }

function make(label) { return class extends Base { constructor(v) { super(v); this.label = label; } }; }
const X = make('x'); const Y = make('y');
Object.setPrototypeOf(X, Alternate);
const x = new X(11); const y = new Y(12);
console.log('evaluations', x.alternate, x.label, y.base, y.label);
class Invalid extends Base { constructor() { super(console.log('evaluated')); } }
Object.setPrototypeOf(Invalid, {});
try { new Invalid(); } catch (e) { console.log('object', e instanceof TypeError); }
Object.setPrototypeOf(Invalid, () => {});
try { new Invalid(); } catch (e) { console.log('arrow-invalid', e instanceof TypeError); }

const proxy = new Proxy(Alternate, { construct(target, args, newTarget) { console.log('construct-trap', newTarget.name); return Reflect.construct(target, args, newTarget); } });
Object.setPrototypeOf(Leaf, proxy);
const proxied = new Leaf(13);
console.log('proxy', proxied.alternate, proxied.target, proxied.leaf);

// A typed constructor specialization still uses the source class's heritage.
class Generic<T> extends Base { constructor(value: T) { super(value); } }
console.log('generic-before', new Generic<number>(7).base);
Object.setPrototypeOf(Generic, class { constructor(value) { this.value = value + 100; } });
console.log('generic-after', new Generic<number>(7).value);

class GenericSpread<T> extends Base { constructor(...args: T[]) { super(...args); } }
Object.setPrototypeOf(GenericSpread, class { constructor(...args) { this.count = args.length; } });
console.log('generic-spread', new GenericSpread<number>(1, 2, 3).count);

// Distinct imported default constructors share the stub sentinel id, not identity.
const { ImportedFirst, ImportedSecond } = require('./_helpers/sp-live-super-heritage/imported.cts');
console.log('imports', new ImportedFirst(5).first, new ImportedSecond(6).second);
Object.setPrototypeOf(ImportedFirst, class { constructor(value) { this.value = value + 20; } });
console.log('import-relink', new ImportedFirst(5).value, new ImportedSecond(6).value);

class Throws { constructor() { throw new Error('spread parent'); } }
class SpreadThrow extends Throws { constructor(...args) { super(...args); } }
Object.setPrototypeOf(SpreadThrow, Throws);
try { new SpreadThrow(1); } catch (e) { console.log('spread-throw', e.message); }

class ReturnsObject { constructor() { return { replacement: 99 }; } }
class ReplacementChild extends ReturnsObject { own = 4; constructor() { super(); } }
Object.setPrototypeOf(ReplacementChild, ReturnsObject);
const replacementChild = new ReplacementChild();
console.log('checked-replacement', replacementChild.replacement, replacementChild.own);
class ReturnsPrimitive { constructor() { this.kept = 1; return 3; } }
class PrimitiveChild extends ReturnsPrimitive { constructor() { super(); this.own = 2; } }
Object.setPrototypeOf(PrimitiveChild, ReturnsPrimitive);
try { const primitiveChild = new PrimitiveChild(); console.log('checked-primitive', primitiveChild.kept, primitiveChild.own); }
catch (e) { console.log('checked-primitive-error', e.name); }
class TargetBase { constructor() { this.target = new.target.name; } }
class TargetChild extends TargetBase { constructor() { super(); } }
class TargetCustom {}
Object.setPrototypeOf(TargetChild, TargetBase);
console.log('checked-new-target', Reflect.construct(TargetChild, [], TargetCustom).target);

// A nested default construction must recheck after the outer parent relinks it.
let depth = 0;
class ReentrantBase { constructor() { this.kind = 'base'; if (depth === 0) { depth++; Object.setPrototypeOf(ReentrantChild, ReentrantAlternate); this.inner = new NestedChild().kind; } } }
class ReentrantAlternate { constructor() { this.kind = 'alternate'; } }
class ReentrantChild extends ReentrantBase {}
class NestedChild extends ReentrantChild {}
Object.setPrototypeOf(ReentrantChild, ReentrantBase);
const result = new ReentrantChild();
console.log('reentrant-guard', result.kind, result.inner);

const order = [];
class OrderBase { baseField = order.push('base-field'); constructor() { order.push('base-body'); } }
class OrderChild extends OrderBase { childField = order.push('child-field'); constructor() { super(); order.push('child-body'); } }
Object.setPrototypeOf(OrderChild, OrderBase);
new OrderChild();
console.log('field-order-matching', order.join(','));
class OrderAlternate { altField = order.push('alt-field'); constructor() { order.push('alt-body'); } }
order.length = 0;
Object.setPrototypeOf(OrderChild, OrderAlternate);
new OrderChild();
console.log('field-order-changed', order.join(','));

function ThrowingFunctionParent() { throw new Error("function-parent"); }
class FunctionThrowChild extends Base { constructor() { super(); } }
Object.setPrototypeOf(FunctionThrowChild, ThrowingFunctionParent);
try { new FunctionThrowChild(); } catch (error) { console.log("function-parent-throw", error.message); }
