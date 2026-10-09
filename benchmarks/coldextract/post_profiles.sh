#!/bin/bash
set -e
cd /root/lanes/perry-coldextract
while [ ! -f final-gate.rc ]; do sleep 15; done
[ "$(cat final-gate.rc)" = 0 ]
mkdir -p profiles-final
for mode in write-sync write-async hash file-hash verify verdict spool; do
 for arm in base fix node; do
  if [ "$mode" = spool ]; then b=spool; src=micro/spool.ts; args=(2); else b=micro; src=micro/driver.ts; args=("$mode" 1); fi
  if [ "$arm" = node ]; then cmd=(/root/lanes/11842/node-v24.9.0-linux-x64/bin/node "$src"); else cmd=(bins-$arm/$b); fi
  taskset -c 0-55 setarch -R strace -c -f -o profiles-final/$mode-$arm.strace "${cmd[@]}" "${args[@]}" > profiles-final/$mode-$arm.out 2>profiles-final/$mode-$arm.err
  rm -rf work/micro-*
 done
done
flock /root/MEASURE.lock bash -c '
 set -e
 echo "perry-coldextract/final attribution $(date -u +%FT%TZ)" > /root/MEASURE.holder
 for mode in write-sync write-async hash file-hash verify verdict spool; do
  for arm in base fix; do
   if [ "$mode" = spool ]; then b=spool; args=(20); else b=micro; args=("$mode" 20); fi
   taskset -c 56-63 setarch -R perf record -q -e cycles:u -F 499 --call-graph dwarf,8192 -o profiles-final/$mode-$arm.data bins-$arm/$b "${args[@]}" >profiles-final/$mode-$arm.profile.out 2>profiles-final/$mode-$arm.profile.err
   rm -rf work/micro-*
   perf report --stdio --no-children --call-graph none --percent-limit 0.5 -i profiles-final/$mode-$arm.data > profiles-final/$mode-$arm.self
   perf report --stdio --no-children --percent-limit 0.5 -i profiles-final/$mode-$arm.data > profiles-final/$mode-$arm.report
  done
 done
'
python3 summarize_lane.py > summary.log
echo 0 > post-profiles.rc
