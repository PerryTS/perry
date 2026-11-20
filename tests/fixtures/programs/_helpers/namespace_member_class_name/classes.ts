// Classes whose names equal members of an unrelated namespace (values.ts).
const Base = (id: string) =>
  class {
    static make(input: unknown) {
      return "Base.make " + id + " " + JSON.stringify(input);
    }
  };

// Inherits make from a class-valued parent expression, like effect Schema.Class.
export class Info extends Base("Config.Info") {}

export class Document extends Base("Config.Document") {}

// Declares its own static method.
export class Name {
  static make(input: unknown) {
    return "Name.make " + JSON.stringify(input);
  }
}

export class Shape {
  static tag = "class Shape";
}
