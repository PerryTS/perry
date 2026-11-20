const proto: any = { x: "proto-x", kind: "proto-kind" };
const child: any = Object.create(proto);
child.x = "own-x";
delete child.x;
proto.x = "proto-x2";
console.log(child.x);
