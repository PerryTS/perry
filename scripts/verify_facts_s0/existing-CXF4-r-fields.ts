class Two { r = 1; s = 2; }
class Other { pad = "pad"; }
const nt: any = Reflect.construct(Two, [], Other);
console.log(nt.r, nt.s, nt.pad);
