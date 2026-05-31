import fsDefault, {
  Dir,
  Dirent,
  FileReadStream,
  FileWriteStream,
  ReadStream,
  Stats,
  WriteStream,
  _toUnixTimestamp,
} from "node:fs";
import * as fs from "node:fs";

const ROOT = "/tmp/perry_node_suite_fs_namespace_exports";
try {
  fs.rmSync(ROOT, { recursive: true, force: true });
} catch (_e) {}
fs.mkdirSync(ROOT, { recursive: true });

const file = ROOT + "/file.txt";
fs.writeFileSync(file, "hello");

for (const name of [
  "Dir",
  "Dirent",
  "Stats",
  "ReadStream",
  "WriteStream",
  "FileReadStream",
  "FileWriteStream",
  "_toUnixTimestamp",
]) {
  const descriptor = Object.getOwnPropertyDescriptor(fs, name);
  console.log(
    `key ${name}:`,
    Object.keys(fs).includes(name),
    Object.prototype.propertyIsEnumerable.call(fs, name),
    descriptor?.enumerable,
  );
  const defaultDescriptor = Object.getOwnPropertyDescriptor(fsDefault, name);
  console.log(
    `default descriptor ${name}:`,
    Object.keys(fsDefault).includes(name),
    Object.prototype.propertyIsEnumerable.call(fsDefault, name),
    defaultDescriptor?.enumerable,
    defaultDescriptor?.configurable,
  );
}

const existingDescriptor = Object.getOwnPropertyDescriptor(fs, "readFileSync");
const existingDefaultDescriptor = Object.getOwnPropertyDescriptor(
  fsDefault,
  "readFileSync",
);
const openAsBlobDescriptor = Object.getOwnPropertyDescriptor(fs, "openAsBlob");
console.log(
  "existing readFileSync export:",
  Object.keys(fs).includes("readFileSync"),
  Object.prototype.propertyIsEnumerable.call(fs, "readFileSync"),
  !!existingDescriptor,
  existingDescriptor?.enumerable,
  typeof existingDescriptor?.value,
);
console.log(
  "default existing readFileSync export:",
  Object.keys(fsDefault).includes("readFileSync"),
  Object.prototype.propertyIsEnumerable.call(fsDefault, "readFileSync"),
  !!existingDefaultDescriptor,
  existingDefaultDescriptor?.enumerable,
  existingDefaultDescriptor?.configurable,
  typeof existingDefaultDescriptor?.value,
);
console.log(
  "openAsBlob export:",
  Object.keys(fs).includes("openAsBlob"),
  Object.prototype.propertyIsEnumerable.call(fs, "openAsBlob"),
  !!openAsBlobDescriptor,
  openAsBlobDescriptor?.enumerable,
  typeof openAsBlobDescriptor?.value,
  fs.openAsBlob.length,
);

function descriptorKind(name: string) {
  const descriptor = Object.getOwnPropertyDescriptor(fsDefault, name);
  console.log(
    `default descriptor kind ${name}:`,
    Object.prototype.hasOwnProperty.call(descriptor ?? {}, "value"),
    typeof descriptor?.value,
    Object.prototype.hasOwnProperty.call(descriptor ?? {}, "get"),
    typeof descriptor?.get,
    Object.prototype.hasOwnProperty.call(descriptor ?? {}, "set"),
    typeof descriptor?.set,
    descriptor?.enumerable,
    descriptor?.configurable,
    descriptor?.writable,
  );
}

for (const name of [
  "ReadStream",
  "WriteStream",
  "FileReadStream",
  "FileWriteStream",
  "promises",
  "constants",
]) {
  descriptorKind(name);
}

console.log(
  "named functions:",
  [
    Dir,
    Dirent,
    Stats,
    ReadStream,
    WriteStream,
    FileReadStream,
    FileWriteStream,
    _toUnixTimestamp,
  ].every((value) => typeof value === "function"),
);
console.log(
  "default identity:",
  fsDefault.ReadStream === ReadStream,
  fsDefault._toUnixTimestamp === _toUnixTimestamp,
);
console.log(
  "aliases:",
  fs.FileReadStream === fs.ReadStream,
  fs.FileWriteStream === fs.WriteStream,
  FileReadStream === ReadStream,
  FileWriteStream === WriteStream,
);
console.log(
  "lengths:",
  Dir.length,
  Dirent.length,
  Stats.length,
  ReadStream.length,
  WriteStream.length,
  _toUnixTimestamp.length,
);
console.log(
  "names:",
  Dir.name,
  Dirent.name,
  ReadStream.name,
  WriteStream.name,
  _toUnixTimestamp.name,
);

console.log("timestamp number:", _toUnixTimestamp(123));
console.log("timestamp string:", _toUnixTimestamp("123.5"));
console.log("timestamp hex:", _toUnixTimestamp("0x10"));
console.log("timestamp date:", _toUnixTimestamp(new Date(1500)));
const negativeCurrent = _toUnixTimestamp(-1);
console.log(
  "timestamp negative current:",
  typeof negativeCurrent === "number" && negativeCurrent > 1_000_000_000,
);

for (const bad of [NaN, Infinity, {}, "abc", null, undefined, true]) {
  try {
    _toUnixTimestamp(bad as never);
    console.log("bad ok");
  } catch (e: any) {
    console.log("bad error:", e instanceof TypeError, e.code);
  }
}

const stat = fs.statSync(file);
const dirent = fs.readdirSync(ROOT, { withFileTypes: true })[0];
const dir = fs.opendirSync(ROOT);
console.log("instance stats:", stat instanceof fs.Stats, stat instanceof Stats);
console.log("instance dirent:", dirent instanceof fs.Dirent, dirent instanceof Dirent);
console.log("instance dir:", dir instanceof fs.Dir, dir instanceof Dir);
dir.closeSync();

const rs = fs.createReadStream(file);
console.log(
  "instance readstream:",
  rs instanceof fs.ReadStream,
  rs instanceof fs.FileReadStream,
  rs instanceof ReadStream,
);
rs.destroy();

const ws = fs.createWriteStream(file);
console.log(
  "instance writestream:",
  ws instanceof fs.WriteStream,
  ws instanceof fs.FileWriteStream,
  ws instanceof WriteStream,
);
ws.destroy();
