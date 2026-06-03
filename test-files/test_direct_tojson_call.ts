class Fixture {
  value = "deploy";

  toJSON(): any {
    return { name: this.value };
  }

  describe(): any {
    return { label: this.value + "-ok" };
  }
}

const fixture: any = new Fixture();
const stringifyActual = JSON.stringify(fixture);
const stringifyExpected = '{"name":"deploy"}';
const actual = JSON.stringify({
  direct: fixture.toJSON(),
  bracket: fixture["toJSON"](),
  computed: fixture["to" + "JSON"](),
  callResult: fixture.toJSON.call(fixture),
  control: fixture.describe(),
});
const expected =
  '{"direct":{"name":"deploy"},"bracket":{"name":"deploy"},"computed":{"name":"deploy"},"callResult":{"name":"deploy"},"control":{"label":"deploy-ok"}}';

if (actual !== expected) {
  console.log(actual);
  throw new Error("direct toJSON call result did not match bracket/computed/call forms");
}

if (stringifyActual !== stringifyExpected) {
  console.log(stringifyActual);
  throw new Error("JSON.stringify no longer honors userland toJSON");
}

console.log(actual);
