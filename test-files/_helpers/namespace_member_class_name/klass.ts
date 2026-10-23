class Parent {
  static inherited() {
    return "Klass.inherited";
  }
}

export class Klass extends Parent {
  static make(n: number) {
    return "Klass.make " + n;
  }
}
