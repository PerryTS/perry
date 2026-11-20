### Bun compatibility

- **`Bun.serve` and `import { serve } from "bun"` now run on Perry's native
  HTTP server.** The facade supports ephemeral ports, hostname binding,
  Fetch `Request`/`Response` handlers (including async returns and the `error`
  callback), `requestIP`, observable server properties, and
  `stop`/`ref`/`unref` lifecycle methods. TLS options fail explicitly with
  `ERR_NOT_SUPPORTED` until native TLS support is added.
