Dead-dynamic-import pruning (DCE layer 2) now walks the module scope of every
module a site-loaded module re-exports from. Before, a module only loaded
through `export ... from` inside an `import()` target was never walked, so its
own `import()` sites could look dead and their targets could be dropped from
the binary. The activation pass resolves re-export sources the same way the
site-loaded set does (one `reexport_target`), and its points-to bound is
raised to 1e9 entries so the Supabase CLI still converges with the bodies it
now has to walk. A wpfacts fixture fails when the re-export activation is
removed.
