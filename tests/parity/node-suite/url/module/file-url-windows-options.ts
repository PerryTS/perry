import { fileURLToPath, fileURLToPathBuffer, pathToFileURL } from "node:url";

function show(label: string, fn: () => unknown) {
  try {
    console.log(label + ":", JSON.stringify(fn()));
  } catch (err: any) {
    console.log(label + ":", err?.name, err?.code || "no-code");
  }
}

show("file win drive", () => fileURLToPath("file:///C:/path/with%20space", { windows: true }));
show("file win unc", () => fileURLToPath("file://server/share/f.txt", { windows: true }));
show("file posix unc", () => fileURLToPath("file://server/share/f.txt", { windows: false }));
show("file win encoded slash", () => fileURLToPath("file:///tmp/a%2Fb", { windows: true }));
show("file win encoded backslash", () => fileURLToPath("file:///tmp/a%5Cb", { windows: true }));
show("file posix encoded backslash", () => fileURLToPath("file:///tmp/a%5Cb", { windows: false }));

show("buffer posix invalid bytes", () =>
  fileURLToPathBuffer("file:///tmp/a%ff", { windows: false }).toString("hex")
);
show("buffer win drive", () =>
  fileURLToPathBuffer("file:///C:/path/with%20space", { windows: true }).toString("hex")
);

show("path win drive", () => pathToFileURL("C:\\path\\a b", { windows: true }).href);
show("path win unc", () => pathToFileURL("\\\\server\\share\\f.txt", { windows: true }).href);
show("path posix win-looking", () => pathToFileURL("C:\\path\\a b", { windows: false }).href);
