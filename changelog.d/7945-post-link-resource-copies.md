**Standalone resource staging stops copying an unrelated ancestor's asset
tree after linking (#6899).** The post-link staging walk looked upward for
`package.json` only, so a perry.toml-anchored app nested under another
package selected the wrong project root and recursively copied its
`logo`/`assets`/`resources`/`images` — presenting as a silent post-link
hang. Staging now routes through the shared project-root/copy helpers,
`perry.toml` anchors the walk like `package.json`, and an unanchored search
falls back to the starting directory as its contract always said.
(Fragment added at merge; full analysis in the PR body.)
