# Embedded Bun text-loader require

Independent regression for `createRequire` loading an embedded text module.
The manifest explicitly marks `message.md` and an
empty `.txt` as Bun Loader::Text (13); another `.md` remains an ordinary asset.
No Claude source, credentials, or API server is required.

Compile `main.js` with `--platform bun --bunfs-root` pointing at this directory,
then run the resulting executable from a different directory. The fixture
checks exact UTF-8 text, missing files, empty contents, cache/GC survival,
`require.resolve`, and a virtual-base `createRequire` with relative components.
An ordinary asset is not implicitly evaluated or reclassified by extension.

Before the fix, filesystem reads of the embedded Markdown succeed but its
require throws MODULE_NOT_FOUND. This is not a test of generic dynamic JS
evaluation, native addons, JSON loading, or Bun's Markdown-to-HTML loader.

