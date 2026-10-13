import { replaceStaticPrototype } from "./_helpers/class_virtual_certificate_mutator.ts";
class StaticCertBase { scale: number; constructor(k: number) { this.scale = k; } step(x: number): number { return (x & 255) * this.scale + 7; } }
class StaticCertDerived extends StaticCertBase { step(x: number): number { return (x & 255) * this.scale + 11; } }
function staticCertificateKernel(): number {
  const cs: StaticCertBase[] = [new StaticCertBase(3), new StaticCertDerived(5)];
  let sum = 0;
  for (let i = 0; i < 40; i++) {
    if (i === 20) replaceStaticPrototype(Object.getPrototypeOf(cs[0]));
    sum += cs[i & 1].step(i);
  }
  return sum;
}
console.log('static-epoch', staticCertificateKernel());
