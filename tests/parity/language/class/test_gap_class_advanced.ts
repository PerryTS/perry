// Test advanced class features Perry doesn't support
// Expected output:
// private method: 42
// private static: helper result
// private getter: 100
// private setter: 200
// static block initialized: true
// field init no constructor: 5 hello
// base throws: Method not implemented
// subclass override: 42
// mixin says hello: Hi from Mixin
// mixin original: base value
// class expression: from expression class
// arguments: 1 2 3
// tagged template: strings=Hello ,,,! values=world,42
// new.target: MyConstructable
// new.target undefined for call: true

// --- Private methods ---
class WithPrivateMethod {
  #secret(): number {
    return 42;
  }
  reveal(): number {
    return this.#secret();
  }
}
const wpm = new WithPrivateMethod();
console.log("private method: " + wpm.reveal());

// --- Private static methods ---
class WithPrivateStatic {
  static #helper(): string {
    return "helper result";
  }
  static publicMethod(): string {
    return WithPrivateStatic.#helper();
  }
}
console.log("private static: " + WithPrivateStatic.publicMethod());

// --- Private getters/setters ---
class WithPrivateAccessor {
  #_value: number = 100;
  get #value(): number {
    return this.#_value;
  }
  set #value(v: number) {
    this.#_value = v;
  }
  getValue(): number {
    return this.#value;
  }
  setValue(v: number): void {
    this.#value = v;
  }
}
const wpa = new WithPrivateAccessor();
console.log("private getter: " + wpa.getValue());
wpa.setValue(200);
console.log("private setter: " + wpa.getValue());

// --- Static class blocks ---
class WithStaticBlock {
  static initialized: boolean;
  static {
    WithStaticBlock.initialized = true;
  }
}
console.log("static block initialized: " + WithStaticBlock.initialized);

// --- Class field declarations with initializers (no constructor) ---
class FieldInit {
  x: number = 5;
  y: string = "hello";
}
const fi = new FieldInit();
console.log("field init no constructor: " + fi.x + " " + fi.y);

// --- Abstract-like pattern: base throws, subclass overrides ---
class BaseAbstract {
  compute(): number {
    throw new Error("Method not implemented");
  }
}
class ConcreteImpl extends BaseAbstract {
  compute(): number {
    return 42;
  }
}
const base = new BaseAbstract();
try {
  base.compute();
} catch (e: any) {
  console.log("base throws: " + e.message);
}
const concrete = new ConcreteImpl();
console.log("subclass override: " + concrete.compute());

// --- Mixin pattern ---
type Constructor = new (...args: any[]) => any;
function Greetable<T extends Constructor>(Base: T) {
  return class extends Base {
    greet(): string {
      return "Hi from Mixin";
    }
  };
}
class BaseClass {
  value: string = "base value";
}
const MixedClass = Greetable(BaseClass);
const mixed = new MixedClass();
console.log("mixin says hello: " + mixed.greet());
console.log("mixin original: " + mixed.value);

// --- Class expression ---
const ExprClass = class {
  message: string;
  constructor(msg: string) {
    this.message = msg;
  }
  getMessage(): string {
    return this.message;
  }
};
const ec = new ExprClass("from expression class");
console.log("class expression: " + ec.getMessage());

// --- arguments object in regular function ---
function showArguments(): void {
  console.log("arguments: " + Array.from(arguments).join(" "));
}
showArguments(1, 2, 3);

// --- Tagged template literals ---
function tag(strings: TemplateStringsArray, ...values: any[]): string {
  console.log("tagged template: strings=" + Array.from(strings).join(",") + " values=" + values.join(","));
  return "tagged";
}
const name = "world";
tag`Hello ${name},${42}!`;

// --- new.target inside constructor ---
class MyConstructable {
  constructorName: string;
  constructor() {
    this.constructorName = new.target ? new.target.name : "undefined";
  }
}
const mc = new MyConstructable();
console.log("new.target: " + mc.constructorName);

// new.target is undefined when called as regular function
function checkNewTarget(): boolean {
  return new.target === undefined;
}
console.log("new.target undefined for call: " + checkNewTarget());
