import { ImportedCommandBuilder } from "./fixtures/direct_tojson_pkg/builder.ts";

const command: any = new ImportedCommandBuilder().setValue("deploy");
const actual = JSON.stringify({
  direct: command.toJSON(),
  bracket: command["toJSON"](),
  computed: command["to" + "JSON"](),
  callResult: command.toJSON.call(command),
});
const expected =
  '{"direct":{"name":"deploy"},"bracket":{"name":"deploy"},"computed":{"name":"deploy"},"callResult":{"name":"deploy"}}';

if (actual !== expected) {
  console.log(actual);
  throw new Error("imported builder direct toJSON call did not match bracket/computed/call forms");
}

console.log(actual);
