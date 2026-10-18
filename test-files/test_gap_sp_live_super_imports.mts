import * as Ns from './_helpers/sp-live-super-heritage/namespace.mts';
console.log('namespaces', new Ns.First(3).kind, new Ns.Second(4).kind);
Object.setPrototypeOf(Ns.First, class { constructor(value) { this.value = value + 20; } });
console.log('namespace-relink', new Ns.First(3).value, new Ns.Second(4).value);
