// A self-namespace whose members are plain values that share class names.
export * as Values from "./values.ts";
export { Klass } from "./klass.ts";

function schema(id: string) {
  function Schema() {}
  const self = Schema as unknown as { make: (input: object) => object; id: string };
  self.make = (input: object) => ({ id, ...input });
  self.id = id;
  return self;
}

export const Info = schema("Skill.Info");
export const Name = schema("Skill.Name");
export const Shape = { kind: "object Shape" };
