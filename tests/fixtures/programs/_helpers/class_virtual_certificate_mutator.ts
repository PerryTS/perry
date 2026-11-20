function replacement(x: number): number { return 100 + x; }
export function replaceStaticPrototype(proto: any): void { proto.step = replacement; }
