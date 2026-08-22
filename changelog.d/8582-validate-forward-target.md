Fixed guarded array reads so a selected forwarding target is validated as a
heap address before Perry dereferences its header. Invalid or longer forwarding
chains now route to the boxed fallback instead of permitting a speculative
native header load.
