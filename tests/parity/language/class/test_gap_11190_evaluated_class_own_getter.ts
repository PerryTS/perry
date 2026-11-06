// #11190 / #11142: a class declared in a function with a runtime `extends`
// value is evaluated per call. Its own ClassBody getters must shadow the
// evaluated parent's getters. That holds for instances built outside the
// body, and for instances a static factory builds with `new Self()` inside
// the body.
//
// This is luxon's zone shape: `class FixedOffsetZone extends Zone` with a
// `static get utcInstance()` singleton factory, overriding getters that the
// abstract `Zone` base defines to throw "Zone is an abstract class". cron's
// `CronTime` hit it on the first tick.

class ZoneIsAbstractError extends Error {
  constructor() {
    super("Zone is an abstract class");
  }
}

class Zone {
  get type(): string {
    throw new ZoneIsAbstractError();
  }
  get isValid(): boolean {
    throw new ZoneIsAbstractError();
  }
  offset(_ts: number): number {
    throw new ZoneIsAbstractError();
  }
}

function zones(Base: any) {
  let singleton: any = null;
  class FixedOffsetZone extends Base {
    fixed: number;
    static get utcInstance() {
      if (singleton === null) {
        singleton = new FixedOffsetZone(0);
      }
      return singleton;
    }
    static instance(offset: number) {
      return offset === 0 ? FixedOffsetZone.utcInstance : new FixedOffsetZone(offset);
    }
    constructor(offset: number) {
      super();
      this.fixed = offset;
    }
    get type() {
      return "fixed";
    }
    get isValid() {
      return true;
    }
    offset() {
      return this.fixed;
    }
  }
  const cache = new Map<string, any>();
  class IANAZone extends Base {
    #name: string;
    static create(name: string) {
      let zone = cache.get(name);
      if (zone === undefined) {
        cache.set(name, (zone = new IANAZone(name)));
      }
      return zone;
    }
    constructor(name: string) {
      super();
      this.#name = name;
    }
    get type() {
      return "iana";
    }
    get isValid() {
      return this.#name.includes("/");
    }
    get name() {
      return this.#name;
    }
  }
  return { FixedOffsetZone, IANAZone };
}

function t(label: string, f: () => unknown) {
  try {
    console.log(label, String(f()));
  } catch (e) {
    console.log(label, "threw", (e as Error).message);
  }
}

const { FixedOffsetZone, IANAZone } = zones(Zone);
t("utc.type", () => FixedOffsetZone.utcInstance.type);
t("utc.isValid", () => FixedOffsetZone.utcInstance.isValid);
t("utc.offset", () => FixedOffsetZone.utcInstance.offset(0));
t("instance(60)", () => FixedOffsetZone.instance(60).type + ":" + FixedOffsetZone.instance(60).offset(0));
t("outer new", () => new FixedOffsetZone(5).type);
t("outer isValid", () => new FixedOffsetZone(5).isValid);
t("iana", () => IANAZone.create("Europe/Berlin").type + ":" + IANAZone.create("Europe/Berlin").name);
t("iana.isValid", () => IANAZone.create("Europe/Berlin").isValid + "," + IANAZone.create("bogus").isValid);
t("instanceof", () => [FixedOffsetZone.utcInstance instanceof Zone, FixedOffsetZone.utcInstance instanceof FixedOffsetZone].join(","));
t("proto", () => Object.getPrototypeOf(FixedOffsetZone.utcInstance) === FixedOffsetZone.prototype);

// A second evaluation has its own singleton and its own parent.
class OtherZone {
  get type(): string {
    return "other-base";
  }
  get isValid(): boolean {
    return false;
  }
}
const second = zones(OtherZone);
t("second.type", () => second.FixedOffsetZone.utcInstance.type);
t("second.distinct", () => second.FixedOffsetZone.utcInstance !== FixedOffsetZone.utcInstance);
t("second.instanceof", () => [second.FixedOffsetZone.utcInstance instanceof OtherZone, second.FixedOffsetZone.utcInstance instanceof Zone].join(","));

// A subclass that does NOT override a getter still reaches the evaluated
// parent's one.
function plain(Base: any) {
  class NoOverride extends Base {
    static make() {
      return new NoOverride();
    }
  }
  return NoOverride;
}
t("no-override", () => plain(OtherZone).make().type);
t("no-override outer", () => new (plain(OtherZone))().isValid);
