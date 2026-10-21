import { readFileSync } from "node:fs";
import { collect } from "perry/gc";
import { load } from "./generated/bindings";
import type { HostResources } from "./generated/bindings";

const bytes = readFileSync(process.argv[2]);
const dropped: number[] = [];
const original = { reason: "host resource destructor failed" };
function check(ok: boolean, message: string): void { if (!ok) throw new Error(message); }
function rejects(action: () => unknown, message: string): void {
  let rejected = false;
  try { action(); } catch (_) { rejected = true; }
  check(rejected, message);
}
function setup(input = bytes) {
  let factories!: HostResources;
  let escaped: unknown;
  const component = load(input, (resources) => {
    factories = resources;
    const token = resources["perry:host-resources/tokens@1.0.0"].token;
    // Factories must work before engine instantiation. This value is also
    // cleaned up if the component bytes fail validation.
    token.create({ value: 900 }, (payload) => {
      collect(); dropped.push((payload as { value: number }).value);
    });
    return {
      "perry:host-resources/tokens@1.0.0": {
        "[constructor]token"(value: number) {
          collect();
          return token.create({ value }, (payload) => {
            collect();
            const value = (payload as { value: number }).value;
            dropped.push(value);
            if (value === 13) throw original;
          });
        },
        "[method]token.read"(value) {
          escaped = value;
          collect();
          return (token.value(value) as { value: number }).value;
        }
      },
      "perry:host-resources/aliases@1.0.0": {
        "read-alias"(value) {
          collect();
          return (resources["perry:host-resources/aliases@1.0.0"].alias.value(value) as { value: number }).value;
        }
      }
    };
  });
  return { component, factories, escaped: () => escaped };
}

const a = setup();
const api = a.component.exports["perry:host-resources/runner@1.0.0"];
const token = a.factories["perry:host-resources/tokens@1.0.0"].token;
const first = api.make(42);
const borrow = first.borrow();
collect();
check(api.read(borrow) === 42, "borrow payload");
check(api["read-alias"](borrow) === 42, "nominal alias payload");
rejects(() => token.value(a.escaped() as Parameters<typeof token.value>[0]), "callback borrow escaped");
rejects(() => api["consume-pair"]({ first, second: first }), "duplicate own accepted");
rejects(() => api.mixed(first, borrow), "own and borrow alias accepted");
rejects(() => api.checked(first, 999), "invalid trailing argument accepted");
check(api.read(borrow) === 42, "failed lowering consumed resource");
const returned = api.echo(first);
rejects(() => api.read(borrow), "old borrow revived");
rejects(() => api.echo(first), "old owner revived");
check(api.read(returned.borrow()) === 42, "roundtrip payload");
const b = setup();
rejects(() => b.component.exports["perry:host-resources/runner@1.0.0"].read(returned.borrow()), "foreign instance accepted");
const other = a.factories["perry:host-resources/tokens@1.0.0"].other.create({ value: 1 });
rejects(() => api.read(other as unknown as Parameters<typeof api.read>[0]), "wrong nominal type accepted");
other.dispose();
rejects(() => api.read({ $hostResource: "1", owned: false } as unknown as Parameters<typeof api.read>[0]), "forged wrapper accepted");
api.keep(returned);
for (let i = 0; i < 50; i++) {
  const temporary = { text: "allocation " + i, data: [i, i + 1, i + 2] };
  collect();
  check(temporary.data[0] === i, "temporary moved incorrectly");
}
const held = api.take();
check(api.read(held.borrow()) === 42, "guest-held payload lost during collection");
api.consume(held);
check(dropped.filter(value => value === 42).length === 1, "guest drop count");
const local = token.create({ value: 7 }, (payload) => { collect(); dropped.push((payload as { value: number }).value); });
local.dispose(); local.dispose();
check(dropped.filter(value => value === 7).length === 1, "local drop count");
const throwing = api.make(13);
let caught: unknown;
try { throwing.dispose(); } catch (error) { caught = error; }
check(caught === original, "destructor exception identity");
throwing.dispose();
a.component.dispose(); a.component.dispose();
b.component.dispose();

const trapped = setup();
const trappedApi = trapped.component.exports["perry:host-resources/runner@1.0.0"];
trappedApi.keep(trappedApi.make(13));
rejects(() => trappedApi.fail(trappedApi.make(88)), "guest trap missing");
caught = undefined;
try { trapped.component.dispose(); } catch (error) { caught = error; }
check(caught === original, "cleanup lost first destructor exception");
trapped.component.dispose();
check(dropped.filter(value => value === 88).length === 1, "cleanup stopped after failing destructor");
check(dropped.filter(value => value === 13).length === 2, "throwing destructor repeated");
const count = dropped.filter(value => value === 900).length;
rejects(() => setup(new Uint8Array([0, 1, 2])), "invalid component accepted");
check(dropped.filter(value => value === 900).length === count + 1, "failed load leaked host payload");
console.log("PASS host resources: aliases, ownership, moving payloads, borrows, rollback, cleanup, original exceptions");
