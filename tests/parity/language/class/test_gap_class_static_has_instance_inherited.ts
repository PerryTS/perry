// A subclass inherits its parent's static Symbol.hasInstance, whether the hook
// is declared in the class body or assigned with Object.defineProperty.
class Even {
  static [Symbol.hasInstance](n: unknown): boolean {
    return typeof n === "number" && n % 2 === 0;
  }
}
class SubEven extends Even {}
class SubSubEven extends SubEven {}

class Odd {}
Object.defineProperty(Odd, Symbol.hasInstance, {
  value: (n: unknown) => typeof n === "number" && n % 2 === 1,
});
class SubOdd extends Odd {}

console.log("Even", 2 instanceof Even, 3 instanceof Even);
console.log("SubEven", 2 instanceof SubEven, 3 instanceof SubEven);
console.log("SubSubEven", 4 instanceof SubSubEven, 5 instanceof SubSubEven);
console.log("Odd", 3 instanceof Odd, 2 instanceof Odd);
console.log("SubOdd", 3 instanceof SubOdd, 2 instanceof SubOdd);
console.log("plain", new SubEven() instanceof SubEven, {} instanceof SubEven);
