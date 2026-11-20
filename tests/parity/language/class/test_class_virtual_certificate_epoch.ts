class CertBase { value = 3; step(x: number): number { return this.value + x; } }
class CertDerived extends CertBase { step(x: number): number { return this.value * x; } }
function certCall(c: CertBase, x: number): number { const cs: CertBase[] = [c]; return cs[0].step(x); }
const certReceivers: CertBase[] = [new CertBase(), new CertDerived()];
for (let i = 0; i < 20; i++) certCall(certReceivers[i & 1], 2);
console.log('before', certCall(certReceivers[0], 2), certCall(certReceivers[1], 2));
const certProto: any = CertBase.prototype;
certProto.step = function(x: number) { return 100 + x; };
console.log('replace', certCall(certReceivers[0], 2), certCall(certReceivers[1], 2));
