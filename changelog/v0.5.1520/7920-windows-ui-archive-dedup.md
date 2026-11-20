**Windows UI archive dedup no longer drops path-qualified native members.**
Rebuilding the deduplicated Windows UI archive flattened member names, so
equal basenames overwrote one another and WebView2LoaderStatic's
`obj/.../*.obj` members (plus part of `winspool.drv`) were silently omitted —
leaving `CreateCoreWebView2EnvironmentWithOptions` and friends undefined at
link. Members are now normalized to unique flat names, `.drv` members are
treated as import-library members alongside `.dll`, and `uxtheme.lib` /
`winspool.lib` / `rpcrt4.lib` come from the canonical system link line.
(Fragment added at merge; see the PR body for the full analysis.)
