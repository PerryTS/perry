// E25-adjacent: `super` in a class method goes through the method's lexical
// home (ClassMethodHome). A super setter and a super method both write
// fields of the child instance after construction.
// WPFACTS: fact F1 readonly Kid.v = 0
// WPFACTS: fact F1 readonly Kid.n = 0
// WPFACTS: kind Kid.v has str
class Base {
  v: number | string = 0;
  n: number = 0;
  set w(x: number | string) { this.v = x; }
  bump(): void { this.n = this.n + 1; }
}
class Kid extends Base {
  go(): void {
    super.w = "s";
    super.bump();
    const later = () => { super.bump(); };
    later();
  }
}
const k = new Kid();
k.go();
console.log(k.v, k.n);
