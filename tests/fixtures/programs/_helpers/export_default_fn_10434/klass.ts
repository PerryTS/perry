// Control: `export default K` for a class.
class Klass {
  static kind = "class-static";
  read() {
    return "class-read";
  }
}
(Klass as any).extra = "class-extra";
export const holder = { Klass };
export default Klass;
