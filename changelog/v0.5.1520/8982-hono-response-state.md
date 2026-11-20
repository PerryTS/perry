Fixed three defects that together broke Hono response state.

- The read half of a compound or logical private-member assignment (`this.#x ||= v`) now lowers through the same brand guard and class-mangled storage key as an ordinary private read, instead of a differently-keyed path.
- A runtime `HeadersInit` record passed through a non-literal `Response` init object is preserved rather than dropped.
- The Fetch default `text/plain;charset=UTF-8` is installed for string bodies without overriding an explicit content type, and pending BodyInit metadata is cleared across reuse.
