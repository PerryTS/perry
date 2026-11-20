// Static methods and fields
class MathHelper {
  static PI: number = 3.14159;
  static double(x: number): number {
    return x * 2;
  }
  static add(a: number, b: number): number {
    return a + b;
  }
}

console.log(MathHelper.double(5));
console.log(MathHelper.add(3, 4));
