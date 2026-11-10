import { Session } from "node:inspector";

const session = new Session();
session.connect();
try {
  await new Promise<void>((resolve, reject) =>
    session.post("Schema.getDomains", (err, value) => {
      if (err) return reject(err);
      const domains = value.domains.map((domain: any) =>
        `${domain.name}@${domain.version}`
      ).sort();
      console.log("count:", domains.length);
      console.log("domains:", domains.join(","));
      resolve();
    })
  );
} finally {
  session.disconnect();
}
