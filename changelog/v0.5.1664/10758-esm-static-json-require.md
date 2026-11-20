Resolve static relative JSON `require()` calls in ESM-shaped TypeScript from
the requiring module's directory. Packages such as MongoDB can now read their
own `package.json` when compiled from source instead of resolving the path from
the process entry and throwing `MODULE_NOT_FOUND` during connection setup.
