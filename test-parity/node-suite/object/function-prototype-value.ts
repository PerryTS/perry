function show(label: string, value: unknown) {
  console.log(label, JSON.stringify(value));
}

function Foo() {}

const fooProto: any = (Foo as any).prototype;
show("function prototype object", [
  typeof fooProto,
  fooProto !== null,
  fooProto.constructor === Foo,
  (Foo as any).prototype === fooProto,
]);

const child: any = Object.create((Foo as any).prototype);
show("object create accepts function prototype", [
  typeof child,
  child !== null,
]);

function Bar() {}
(Bar as any).prototype = Object.create((Foo as any).prototype);
(Bar as any).prototype.constructor = Bar;

const barProto: any = (Bar as any).prototype;
show("assigned function prototype object", [
  typeof barProto,
  barProto !== null,
  (Bar as any).prototype === barProto,
]);

const arrow = () => {};
show("arrow prototype missing", (arrow as any).prototype === undefined);
