class StaticCertBase { scale = 3; step(x: number): number { return (x & 255) * this.scale + 7; } }
class StaticCertDerived extends StaticCertBase { step(x: number): number { return (x & 255) * this.scale + 11; } }
function staticCertificateKernel(): number {
  const cs: StaticCertBase[] = [new StaticCertBase(), new StaticCertDerived()];
  let sum = 0;
  for (let i = 0; i < 40; i++) {
    if (i === 20) {
      const proto: any = StaticCertBase.prototype;
      proto.step = function(x: number) { return 100 + x; };
    }
    sum += cs[i & 1].step(i);
  }
  return sum;
}
console.log('static-epoch', staticCertificateKernel());
