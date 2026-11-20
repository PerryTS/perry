function check(label: string, condition: boolean): void {
  if (!condition) {
    throw new Error(label);
  }
}

check("Object(true).valueOf", Object(true).valueOf() === true);
check("Object(0).valueOf", Object(0).valueOf() === 0);
check("Object string valueOf", Object("some string").valueOf() === "some string");
check("Object no-arg object", typeof Object() === "object");

const existing = { marker: 1 };
check("Object existing identity", Object(existing) === existing);
check("new Object existing identity", new Object(existing) === existing);
check("new Object primitive value", new Object(false).valueOf() === false);

const assignedStringTarget = Object.assign("x", { extra: 7 });
check("assign string target boxes", typeof assignedStringTarget === "object");
check("assign string target value", (assignedStringTarget as any).valueOf() === "x");
check("assign string target prop", (assignedStringTarget as any).extra === 7);

const source: any = { a: 1 };
Object.defineProperty(source, "hidden", { value: 2, enumerable: false });
const assigned = Object.assign({}, source);
check("assign copies enumerable", (assigned as any).a === 1);
check("assign skips non-enumerable", !Object.prototype.hasOwnProperty.call(assigned, "hidden"));

let getterObserved = false;
const getterSource = Object.defineProperty({}, "boom", {
  enumerable: true,
  get() {
    getterObserved = true;
    throw new Error("getter boom");
  },
});
let getterThrew = false;
try {
  Object.assign({}, getterSource);
} catch (e) {
  getterThrew = String((e as Error).message).indexOf("getter boom") >= 0;
}
check("assign getter observed", getterObserved);
check("assign getter abrupt", getterThrew);

const readOnlyTarget = Object.defineProperty({}, "fixed", {
  value: 1,
  writable: false,
});
let readOnlyThrew = false;
try {
  Object.assign(readOnlyTarget, { fixed: 2 });
} catch (_e) {
  readOnlyThrew = true;
}
check("assign read-only target throws", readOnlyThrew);

(String.prototype as any).fromProto = 42;
const wrapped = new String("abc");
check("string wrapper own length", Object.prototype.hasOwnProperty.call(wrapped, "length"));
check("string wrapper length", wrapped.length === 3);
check("string wrapper prototype", Object.getPrototypeOf(wrapped) === String.prototype);
check("string wrapper inherits", (wrapped as any).fromProto === 42);
check("string wrapper tag", Object.prototype.toString.call(wrapped) === "[object String]");
check("string wrapper constructor", wrapped.constructor === String);
check("String.prototype constructor", String.prototype.constructor === String);
check("string wrapper toString", wrapped.toString() === "abc");
delete (String.prototype as any).fromProto;

console.log("object-string-wrappers ok");
