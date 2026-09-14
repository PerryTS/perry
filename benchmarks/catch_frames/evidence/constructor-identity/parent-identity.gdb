set pagination off
set language c
break js_register_class_parent_dynamic
commands
silent
set $bits = (unsigned long long)$xmm0.v2_int64[0]
printf "register parent: class=%u bits=%llx\n", $edi, $bits
if ($bits >> 48) == 0x7ffd
set $header = $bits & 0xffffffffffff
set $func = *(unsigned long long*)$header
printf "closure function=%llx\n", $func
info symbol $func
set $base = (unsigned long long)&js_register_class_parent_dynamic - 0x747380
if $func == $base + 0x6e2b00
set *(unsigned long long*)$header = $base + 0x41de30
printf "replaced only the duplicated noop identity with the comparison target\n"
end
end
continue
end
run
