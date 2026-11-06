function line(label: string, value: unknown) {
  console.log(label + ":", value);
}

let numberEnumCount = 0;
for (const _key in Number) {
  numberEnumCount++;
}
line("number enum count", numberEnumCount);
let deleteNumberNaN: unknown;
try {
  deleteNumberNaN = delete Number.NaN;
} catch (err) {
  deleteNumberNaN = err instanceof TypeError ? false : "unexpected";
}
line("delete Number.NaN", deleteNumberNaN);
line("Number.NaN present", typeof Number.NaN !== "undefined");

const literalObj = {};
line("Object.prototype literal", Object.prototype.isPrototypeOf(literalObj));

function FooObj() {}
const firstFoo = new (FooObj as any)();
const protoObj = {};
line("Object.prototype first foo", Object.prototype.isPrototypeOf(firstFoo));
line("Foo.prototype first foo", (FooObj as any).prototype.isPrototypeOf(firstFoo));
line("protoObj first foo before", protoObj.isPrototypeOf(firstFoo));
(FooObj as any).prototype = protoObj;
line("protoObj first foo after", protoObj.isPrototypeOf(firstFoo));
const secondFoo = new (FooObj as any)();
line("Object.prototype second foo", Object.prototype.isPrototypeOf(secondFoo));
line("protoObj second foo", protoObj.isPrototypeOf(secondFoo));

const numInstance = new Number();
line("new Number constructor", (numInstance as any).constructor === Number);

let mathThrowsTypeError = false;
let mathThrownConstructor = false;
try {
  new (Math as any)();
} catch (err) {
  mathThrowsTypeError = err instanceof TypeError;
  mathThrownConstructor = (err as any).constructor === TypeError;
}
line("new Math throws TypeError", mathThrowsTypeError);
line("new Math thrown constructor", mathThrownConstructor);

const postExisting: any = { foo: "bar" };
const postExistingResult = postExisting.foo++;
line("post existing result NaN", Number.isNaN(postExistingResult));
line("post existing stored NaN", Number.isNaN(postExisting.foo));

const postMissing: any = {};
const postMissingResult = postMissing.foo++;
line("post missing result NaN", Number.isNaN(postMissingResult));
line("post missing stored NaN", Number.isNaN(postMissing.foo));
line("post missing has key", "foo" in postMissing);

const preExisting: any = { foo: "bar" };
const preExistingResult = ++preExisting.foo;
line("pre existing result NaN", Number.isNaN(preExistingResult));
line("pre existing stored NaN", Number.isNaN(preExisting.foo));

const preMissing: any = {};
const preMissingResult = ++preMissing.foo;
line("pre missing result NaN", Number.isNaN(preMissingResult));
line("pre missing stored NaN", Number.isNaN(preMissing.foo));
line("pre missing has key", "foo" in preMissing);
