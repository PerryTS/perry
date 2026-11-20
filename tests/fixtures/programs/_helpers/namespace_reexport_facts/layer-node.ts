// A module that re-exports its own namespace (`export * as X from "./self"`),
// as opencode's layer-node.ts does. `makeProvider` is called both from this
// module (one literal shape) and through the namespace members (other shapes).
export * as LayerNode from "./layer-node.ts";

export class ProviderNode {
  info: { name: string | undefined; implementation: unknown; dependencies: unknown[] };
  tag: string | undefined;
  constructor(info: { name: string | undefined; implementation: unknown; dependencies: unknown[] }, tag: string | undefined) {
    this.info = info;
    this.tag = tag;
  }
  replace(replacement: { l: string }): ProviderNode {
    return makeProvider({ name: this.info.name, layer: replacement, deps: [] }, this.tag);
  }
}

interface Input {
  readonly name?: string;
  readonly service?: { readonly key: string };
  readonly layer: { l: string };
  readonly deps: readonly ProviderNode[];
}

function makeProvider(input: Input, tag: string | undefined): ProviderNode {
  if (Array.isArray(input.layer)) throw new Error("layer read the deps slot: " + (input.service?.key ?? input.name));
  return new ProviderNode(
    {
      name: input.service !== undefined ? input.service.key : input.name,
      implementation: input.layer,
      dependencies: [...input.deps],
    },
    tag,
  );
}

export function tags(names: readonly string[]) {
  return {
    make: (tag: string) => (input: Input) => makeProvider(input, names.includes(tag) ? tag : undefined),
  };
}

export function make(input: Input & { readonly tag?: string }): ProviderNode {
  return makeProvider(input, input.tag);
}
