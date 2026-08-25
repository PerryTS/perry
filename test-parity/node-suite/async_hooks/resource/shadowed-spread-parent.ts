// A lexical class with a builtin-looking name must still run its own spread
// constructor path instead of being lowered as a native AsyncResource parent.
class AsyncResource {
  readonly marker: string;
  constructor(...values: string[]) {
    this.marker = `user:${values.join(",")}`;
  }
}

class ShadowedResource extends AsyncResource {
  constructor(...values: string[]) {
    super(...values);
  }
}

console.log(
  "shadowed spread parent:",
  new ShadowedResource("first", "second").marker,
);
