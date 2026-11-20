const proto: any = { m() { return this.v; } };
console.log(proto.m.call({ v: 7 }));
const inst: any = Object.create(proto);
Object.defineProperty(inst, "v", { value: 1, enumerable: false });
inst.m = proto.m.bind(inst);
console.log(inst.m());
