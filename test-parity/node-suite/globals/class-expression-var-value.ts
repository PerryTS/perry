var B = class l {
  static parse(e: any) {
    return "parsed:" + e;
  }
  static selfType() {
    return typeof l;
  }
  parse(e: any) {
    return "instance:" + e;
  }
};

const C = class {
  static parse(e: any) {
    return "const:" + e;
  }
  parse(e: any) {
    return "const-instance:" + e;
  }
};

const D = B;

console.log("typeof B:", typeof B);
console.log("B.parse:", B.parse(1));
console.log("new B:", new B().parse(2));
console.log("B inner self:", B.selfType());
console.log("typeof C:", typeof C);
console.log("C.parse:", C.parse(3));
console.log("new C:", new C().parse(4));
console.log("typeof D:", typeof D);
console.log("D.parse:", D.parse(5));
console.log("new D:", new D().parse(6));
