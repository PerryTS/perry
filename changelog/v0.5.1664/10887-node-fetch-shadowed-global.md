Fixed calls through a lexical `fetch` binding, including node-fetch's default
export, being mistaken for the global Web Fetch API and routing the returned
userland response through native response-handle methods.
