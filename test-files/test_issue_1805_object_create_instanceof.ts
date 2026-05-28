// Refs #1805: `Object.create(Object.getPrototypeOf(instance))` must be
// `instanceof` the original class. Property reads and getter dispatch
// already walked the synthetic-prototype chain correctly (#711 / #809);
// only `instanceof` lagged because it keys off the class-id parent
// chain (`get_parent_class_id`) and the synthetic class id allocated
// by `js_object_create` had no parent edge linking it to the original
// class.
//
// Fix: at synthetic-cid registration time, also record
// `(synthetic_cid → proto_ptr.class_id)` in CLASS_REGISTRY when the
// proto pointer carries a non-zero class_id (the typical case where
// `Object.getPrototypeOf(instance)` returns the instance itself per
// Perry's model). The `js_instanceof` chain walk then reaches the
// original class.
//
// Surfaces in effect's `SchemaAST.annotations`, which clones AST nodes
// via `Object.create(Object.getPrototypeOf(ast),
// Object.getOwnPropertyDescriptors(ast))`. Refs #1758, #809.

class C {
    x = 1;
}
const a = new C();
const a_proto = Object.getPrototypeOf(a);

// Plain Object.create(proto).
const clone1 = Object.create(a_proto);
console.log(clone1 instanceof C);

// The descriptor form.
class TypeLiteral {
    _tag = "TypeLiteral";
}
const ast = new TypeLiteral();
const clone2 = Object.create(
    Object.getPrototypeOf(ast),
    Object.getOwnPropertyDescriptors(ast),
);
console.log(clone2._tag);
console.log(clone2 instanceof TypeLiteral);

// Control: original instance is still instanceof.
console.log(a instanceof C);
console.log(ast instanceof TypeLiteral);

// Chained Object.create — `clone3` should still be instanceof C through
// the synthetic→synthetic chain.
const clone3 = Object.create(Object.getPrototypeOf(clone1));
console.log(clone3 instanceof C);

// Negative: Object.create(null) is NOT instanceof Object (no proto).
const bare: any = Object.create(null);
console.log(bare instanceof C);
