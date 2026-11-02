Prevent cross-module function inlining from moving dynamic `require`, dynamic
`import`, or Worker paths into an importing module, where their relative target
map belongs to a different source file and can produce `MODULE_NOT_FOUND`.
