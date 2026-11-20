import { Session } from "node:inspector";

const session = new Session();
session.connect();
try {
  await new Promise<void>((resolve, reject) =>
    session.post("Runtime.evaluate", {
      expression: '({ alpha: 1, beta: "two" })',
      generatePreview: true,
    }, (err, value) => {
      if (err) return reject(err);
      const result = value.result;
      console.log(
        "remote:",
        result.type,
        result.className,
        result.description,
        typeof result.objectId,
      );
      console.log(
        "preview:",
        result.preview.type,
        result.preview.overflow,
        result.preview.properties.map((p: any) =>
          `${p.name}:${p.type}:${p.value}`
        ).join(","),
      );
      resolve();
    })
  );
} finally {
  session.disconnect();
}
