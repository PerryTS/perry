Declare the Windows filesystem provider's direct dependency on the
`Win32_System_WindowsProgramming` API feature. A focused component-host build
otherwise failed when no unrelated workspace crate enabled that feature.
