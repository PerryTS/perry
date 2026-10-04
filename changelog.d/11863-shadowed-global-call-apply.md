Fixed a local binding named like a builtin global (`function process() {}`,
`const Number = ...`) losing its own `.call`, `.apply` and `.bind`: the
compiler redirected them to the global builtin. turndown's HTML-to-Markdown
conversion now works.
