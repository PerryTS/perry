import { entityKind } from "./entity.ts";

export abstract class BaseSession {
  static readonly [entityKind]: string = "BaseSession";
  tag(): string {
    return "base";
  }
}
