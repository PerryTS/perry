import { fileURLToPath, fileURLToPathBuffer, pathToFileURL } from "node:url";

const backslash = String.fromCharCode(92);
const windowsUncPath =
  backslash + backslash + "server" + backslash + "share" + backslash + "f.txt";

function show(label: string, fn: () => unknown) {
  try {
    console.log(label + ":", JSON.stringify(fn()));
  } catch (err: any) {
    console.log(label + ":", err?.name, err?.code || "no-code");
  }
}

show("fileURLToPath windows drive", () =>
  fileURLToPath("file:///C:/path/with%20space", { windows: true })
);
show("fileURLToPath windows unc", () =>
  fileURLToPath("file://server/share/f.txt", { windows: true })
);
show("fileURLToPath windows encoded slash", () =>
  fileURLToPath("file:///tmp/a%2Fb", { windows: true })
);
show("fileURLToPath windows encoded backslash", () =>
  fileURLToPath("file:///tmp/a%5Cb", { windows: true })
);
show("fileURLToPath posix encoded backslash", () =>
  fileURLToPath("file:///tmp/a%5Cb", { windows: false })
);
show("fileURLToPath posix unc host", () =>
  fileURLToPath("file://server/share/f.txt", { windows: false })
);
show("fileURLToPathBuffer invalid utf8", () =>
  fileURLToPathBuffer("file:///tmp/a%ff", { windows: false }).toString("hex")
);
show("fileURLToPathBuffer windows unc", () =>
  fileURLToPathBuffer("file://server/share/f.txt", { windows: true }).toString("hex")
);
show("pathToFileURL windows drive", () =>
  pathToFileURL("C:\\path\\a b", { windows: true }).href
);
show("pathToFileURL windows unc input", () => windowsUncPath);
show("pathToFileURL windows unc", () =>
  pathToFileURL(windowsUncPath, { windows: true }).href
);
show("pathToFileURL posix win-looking", () =>
  pathToFileURL("/tmp/C:\\path\\a b", { windows: false }).href
);
show("pathToFileURL escape set", () =>
  pathToFileURL("/tmp/~!*'():@&=+$,;[] #?").href
);
