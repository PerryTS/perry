- Add a `perry electron <path>` subcommand that runs an existing Electron app
  natively: it locates the main entry from a package.json directory, a packaged
  macOS `.app` bundle, or an `.asar` archive (unpacked to a cache dir via the new
  in-tree asar reader), compiles the main process with `electron` imports
  redirected to the `packages/electron` compat shim through the new
  `--package-alias FROM=TO` compile flag, and launches the native binary with
  the view rendered in the OS-native webview.
