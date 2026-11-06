import { createServer } from "node:http";

function check(label: string, options: any) {
  try {
    createServer(options);
    console.log(label, "ok");
  } catch (error: any) {
    console.log(label, error.name, error.code);
  }
}

check("null", null);
check("number", 1);
check("max header", { maxHeaderSize: -1 });
check("insecure parser", { insecureHTTPParser: "yes" });
check("join duplicates", { joinDuplicateHeaders: 1 });
check("require host", { requireHostHeader: 1 });
check("request timeout", { requestTimeout: -1 });
check("timeout relation", { requestTimeout: 10, headersTimeout: 11 });
check("upgrade callback", { shouldUpgradeCallback: true });
