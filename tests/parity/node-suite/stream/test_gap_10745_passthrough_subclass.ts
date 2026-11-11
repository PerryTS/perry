import { PassThrough, PassThrough as PT } from "node:stream";

class Direct extends PassThrough {
  _transform(chunk: any, _encoding: string, callback: any) {
    callback(null, "direct:" + String(chunk).toUpperCase());
  }
}

class ImportedAlias extends PT {
  _transform(chunk: any, _encoding: string, callback: any) {
    callback(null, "import-alias:" + String(chunk).toUpperCase());
  }
}

const LocalAlias = PassThrough;
const ClassExpression = class extends LocalAlias {
  _transform(chunk: any, _encoding: string, callback: any) {
    callback(null, "class-expr:" + String(chunk).toUpperCase());
  }
};

class Middle extends PassThrough {}
class Indirect extends Middle {
  _transform(chunk: any, _encoding: string, callback: any) {
    callback(null, "indirect:" + String(chunk).toUpperCase());
  }
}

class DefaultPassThrough extends PassThrough {}

function run(name: string, Constructor: any): Promise<void> {
  return new Promise((resolve) => {
    const stream = new Constructor();
    let output = "";
    stream.on("data", (chunk: any) => (output += String(chunk)));
    stream.on("error", (error: any) => {
      console.log(name, "error", error.code || error.message);
      resolve();
    });
    stream.on("end", () => {
      console.log(name, JSON.stringify(output));
      resolve();
    });
    stream.end("ab");
  });
}

(async () => {
  await run("direct", Direct);
  await run("import-alias", ImportedAlias);
  await run("class-expr", ClassExpression);
  await run("indirect", Indirect);
  await run("default", DefaultPassThrough);
})();
