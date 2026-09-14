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
end
continue
end
run
