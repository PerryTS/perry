import First, { First as NamedFirst, Second, shared, helper } from './fixtures/cjs_flat_class_export_collision/classes.cjs';
console.log(new First().value(), typeof First.Second);
console.log(new Second().value(), shared, helper());
console.log(First === NamedFirst);
