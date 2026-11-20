class Two { r = 1; }
class Other { pad = "pad"; r = "other"; }
const nt: any = Reflect.construct(Two, [], Other);
console.log(nt.r);
