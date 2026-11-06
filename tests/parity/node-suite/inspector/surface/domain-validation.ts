import inspector from "node:inspector";

for (const domain of ["Network", "DOMStorage"] as const) {
  for (const name of Object.keys(inspector[domain]).sort()) {
    try {
      (inspector[domain] as Record<string, Function>)[name]("params");
      console.log(domain, name, "unexpected");
    } catch (cause) {
      const error = cause as { name?: string; code?: string; message?: string };
      console.log(
        domain,
        name,
        error.name,
        error.code,
        error.message?.startsWith(
          'The "params" argument must be of type object.',
        ),
      );
    }
  }
}
