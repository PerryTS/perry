// One class declaration evaluated several times yields distinct class objects;
// a static given to one of them (computed key) is not present on a sibling,
// nor on a class extending the sibling (`key in Sub` walks the actual parent).
const SecKey = "~x/Security";
const make = (id: string, sec?: object) => {
  class Service {
    static key = id;
  }
  if (sec) {
    (Service as any)[SecKey] = SecKey;
    (Service as any).security = sec;
  }
  return Service;
};
const withSecurity = make("a", { bearer: 1 });
const plain = make("b");
const SubPlain = class extends plain {};
const SubSecure = class extends withSecurity {};
console.log(SecKey in withSecurity, SecKey in plain, SecKey in SubPlain, SecKey in SubSecure);
console.log("security" in SubPlain, "security" in SubSecure, "key" in SubPlain);
