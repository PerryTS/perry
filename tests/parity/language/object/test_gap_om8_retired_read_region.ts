// Straight-line repeated reads once used a learned read-region word.
// Class, literal and Object.create births must keep working through families
// and the ordinary probe after that internal ABI is removed.
class Pair {
  constructor(a: number, b: number) { this.a = a; this.b = b; }
}
const proto = { inherited: 7 };
const klass: any = new Pair(11, 13);
const literal: any = { a: 17, b: 19 };
const created: any = Object.create(proto);
created.a = 23;
created.b = 29;
function readPair(o: any): number {
  const a = o.a;
  const b = o.b;
  const again = o.a;
  return a + b + again;
}
console.log(readPair(klass), readPair(literal), readPair(created), created.inherited);
klass.late = 31;
literal.late = 37;
created.late = 41;
console.log(readPair(klass), readPair(literal), readPair(created));
delete klass.a;
klass.a = 43;
delete literal.a;
literal.a = 47;
delete created.a;
created.a = 53;
console.log(readPair(klass), readPair(literal), readPair(created));
console.log(Object.keys(klass).join(","), Object.keys(literal).join(","), Object.keys(created).join(","));
// Dynamic construction and observable inherited reads retain the complete path.
const dynamicCtor: any = new Function("this.a = 59; this.b = 61;");
const dynamic: any = new dynamicCtor();
console.log(readPair(dynamic));
let gets = 0;
const accessor = Object.create({ get a() { gets++; return 67; } });
accessor.b = 71;
console.log(readPair(accessor), gets);
