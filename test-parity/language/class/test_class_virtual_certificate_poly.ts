class PolyBase { scale = 3; step(x: number): number { return this.scale * x + 7; } }
class Poly1 extends PolyBase { step(x: number): number { return this.scale * x + 8; } }
class Poly2 extends PolyBase { step(x: number): number { return this.scale * x + 9; } }
class Poly3 extends PolyBase { step(x: number): number { return this.scale * x + 10; } }
class Poly4 extends PolyBase { step(x: number): number { return this.scale * x + 11; } }
class Poly5 extends PolyBase { step(x: number): number { return this.scale * x + 12; } }
class Poly6 extends PolyBase { step(x: number): number { return this.scale * x + 13; } }
class Poly7 extends PolyBase { step(x: number): number { return this.scale * x + 14; } }
class Poly8 extends PolyBase { step(x: number): number { return this.scale * x + 15; } }
function polymorphic(cs: PolyBase[]): number { let n = 0; for (let i = 0; i < 60; i++) n += cs[i % cs.length].step(i); return n; }
console.log('classes-3', polymorphic([new PolyBase(), new Poly1(), new Poly2()]));
console.log('classes-4', polymorphic([new PolyBase(), new Poly1(), new Poly2(), new Poly3()]));
console.log('classes-9', polymorphic([new PolyBase(), new Poly1(), new Poly2(), new Poly3(), new Poly4(), new Poly5(), new Poly6(), new Poly7(), new Poly8()]));
