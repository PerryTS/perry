function TaggedError<_Self>() {
  return <Tag extends string>(tag: Tag, schema: Record<string, string>) =>
    class {
      readonly _tag: Tag = tag;
      readonly _schema = schema;
    };
}

class MyError extends TaggedError<MyError>()("MyError", { code: "string" }) {
  describe(): string {
    return `${this._tag}(code:${this._schema.code})`;
  }
}

const err = new MyError();
console.log("effect._tag:", err._tag);
console.log("effect._schema.code:", err._schema.code);
console.log("effect.describe:", err.describe());
