class Two { r = 1; }
class RestForward extends Two { constructor(...args: any[]) { super(...args); } }
const nt: any = Reflect.construct(Two, [], RestForward);
console.log(nt.r);
