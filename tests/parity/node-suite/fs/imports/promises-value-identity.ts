import fsDefault, { constants, promises as named } from "node:fs";
import fsBare, { promises as bareNamed } from "fs";
import * as fsNamespace from "node:fs";
import * as fsBareNamespace from "fs";
import promisesDefault, { readFile } from "node:fs/promises";
import promisesBareDefault from "fs/promises";
import * as promisesNamespace from "node:fs/promises";
import * as promisesBareNamespace from "fs/promises";
import { syncBuiltinESMExports } from "node:module";
import * as os from "node:os";
import * as path from "node:path";

const originalDescriptor = Object.getOwnPropertyDescriptor(fsDefault, "promises")!;
const original = fsDefault.promises;
const root = fsDefault.mkdtempSync(path.join(os.tmpdir(), "perry-fs-promises-identity-"));
try {
  const file = path.join(root, "input.txt");
  fsDefault.writeFileSync(file, "identity");
  const replacement = {
    readFile: (_path: string, _encoding: string) => Promise.resolve("synchronized"),
  };
  Object.defineProperty(fsDefault, "promises", {
    value: replacement, writable: true, enumerable: true, configurable: true,
  });
  console.log("named snapshot:", named === original && bareNamed === original);
  syncBuiltinESMExports();
  console.log("named synchronization:", named === replacement && bareNamed === replacement);
  const synchronized = named.readFile(file, "utf8");
  const synchronizedBare = bareNamed.readFile(file, "utf8");
  console.log("synchronized methods:", synchronized instanceof Promise, await synchronized,
    synchronizedBare instanceof Promise, await synchronizedBare);
  console.log("subpath remains original:", promisesDefault === original);
  Object.defineProperty(fsDefault, "promises", originalDescriptor);
  syncBuiltinESMExports();

  console.log("parent aliases:", named === fsDefault.promises &&
    named === fsBare.promises && named === fsNamespace.promises &&
    named === fsBareNamespace.promises && named === bareNamed);
  console.log("subpath defaults:", named === promisesDefault && named === promisesBareDefault);
  console.log("namespace defaults:", named === promisesNamespace.default &&
    named === promisesBareNamespace.default);
  console.log("distinct namespace:", named !== promisesNamespace &&
    named !== promisesBareNamespace && promisesNamespace === promisesBareNamespace);
  console.log("method identity:", named.readFile === readFile &&
    named.readFile === promisesDefault.readFile && named.readFile === promisesNamespace.readFile);
  console.log("constants identity:", named.constants === constants &&
    named.constants === fsDefault.constants && named.constants === promisesNamespace.constants);

  const pending = named.readFile(file, "utf8");
  console.log("promise read:", pending instanceof Promise, await pending);


} finally {
  Object.defineProperty(fsDefault, "promises", originalDescriptor);
  syncBuiltinESMExports();
  fsDefault.rmSync(root, { recursive: true, force: true });
}
