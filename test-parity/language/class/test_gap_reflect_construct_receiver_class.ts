class Two { r = 1; }
class Other { pad = 99; r = 7; }
const nt: any = Reflect.construct(Two, [], Other);
console.log(nt.r, nt.pad, Object.keys(nt).join(","));
