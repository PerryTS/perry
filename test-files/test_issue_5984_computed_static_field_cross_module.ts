import { entityKind } from "./fixtures/issue_5984_pkg/entity.ts";
import { BaseSession } from "./fixtures/issue_5984_pkg/base.ts";

class EffectSession extends BaseSession {
  static override readonly [entityKind]: string = "EffectSession";
  tag(): string {
    return "effect";
  }
}

console.log((BaseSession as any)[entityKind]);
console.log((EffectSession as any)[entityKind]);
console.log(new EffectSession().tag());
