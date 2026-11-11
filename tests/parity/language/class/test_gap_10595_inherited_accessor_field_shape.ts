// #10595: a field a subclass overrides must read as the SUBCLASS's value
// from every read path, not just a direct `obj.field` access — including
// from inside an inherited accessor (`Object.defineProperty` getter/setter),
// a dynamic `obj[computedKey]` read, and across a two-level `extends` chain.
//
// Root cause: a subclass field declaration that shares a name with an
// ancestor field is not deduplicated in the class's packed inline-slot
// layout (ancestor fields first, then the class's own), so the object ends
// up with two inline slots for the same logical property. The compile-time
// typed path (`obj.field` on a statically-known class) already resolves to
// the most-derived slot. Every DYNAMIC name-based lookup (an inherited
// accessor's `this.field`, a computed `obj[key]`) used to return the FIRST
// (ancestor's, uninitialized) slot instead.

class Base {
  tag = "base-tag";
}

Object.defineProperty(Base.prototype, "tagViaGetter", {
  get() {
    return (this as any).tag;
  },
});

const tagSym = Symbol("tagSym");
Object.defineProperty(Base.prototype, tagSym, {
  get() {
    return (this as any).tag;
  },
});

let setterLog = "";
Object.defineProperty(Base.prototype, "tagViaAccessor", {
  get() {
    return (this as any).tag;
  },
  set(v: string) {
    setterLog = (this as any).tag + ":" + v;
  },
});

class Sub extends Base {
  tag = "sub-tag";
}

class SubSub extends Sub {
  tag = "subsub-tag";
}

// A field overridden with a different runtime type than its ancestor.
class SubTyped extends Base {
  tag: any = 42;
}

const base = new Base();
console.log("base.tag", base.tag);
console.log("base.tagViaGetter", (base as any).tagViaGetter);
console.log("base[tagSym]", (base as any)[tagSym]);

const sub = new Sub();
// Direct-read control: the non-accessor path already saw the override
// correctly before this fix, and must keep doing so.
console.log("sub.tag", sub.tag);
console.log("sub.tagViaGetter", (sub as any).tagViaGetter);
console.log("sub[tagSym]", (sub as any)[tagSym]);
const key = "tag";
console.log("sub[computedKey]", (sub as any)[key]);

const subsub = new SubSub();
console.log("subsub.tag", subsub.tag);
console.log("subsub.tagViaGetter", (subsub as any).tagViaGetter);
console.log("subsub[tagSym]", (subsub as any)[tagSym]);

const typed = new SubTyped();
console.log("typed.tag", typed.tag);
console.log("typed.tagViaGetter", (typed as any).tagViaGetter);
console.log("typed[tagSym]", (typed as any)[tagSym]);

(sub as any).tagViaAccessor = "written";
console.log("setterLog", setterLog);
console.log("sub.tag after setter", sub.tag);
