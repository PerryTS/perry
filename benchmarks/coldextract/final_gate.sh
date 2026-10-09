#!/bin/bash
set -e
cd /root/lanes/perry-coldextract
while [ ! -f fix-controller.rc ]; do sleep 15; done
[ "$(cat fix-controller.rc)" = 0 ]
while [ ! -f spool-base.rc ]; do sleep 15; done
[ "$(cat spool-base.rc)" = 0 ]
if [ ! -f micro-inst.rc ]; then
 taskset -c 0-55 setarch -R python3 measure_lane.py micro inst > micro-inst.log 2>&1
 echo 0 > micro-inst.rc
fi
if [ ! -f upm-inst.rc ]; then
 taskset -c 0-55 setarch -R python3 measure_lane.py upm inst > upm-inst.log 2>&1
 echo 0 > upm-inst.rc
fi
while [ ! -f early-programs-inst.rc ]; do sleep 15; done
programs_remaining=$(python3 programs_remaining.py)
if [ -n "$programs_remaining" ]; then
 NAMES="$programs_remaining" taskset -c 0-55 setarch -R python3 measure_lane.py programs inst > programs-inst.log 2>&1
fi
echo 0 > programs-inst.rc
if [ ! -f early-final-cycles.rc ]; then
 bash early_final_cycles.sh
fi

for kind in micro upm programs; do taskset -c 0-55 setarch -R python3 measure_lane.py "$kind" gc > "$kind-gc.log" 2>&1; echo 0 > "$kind-gc.rc"; done
echo 0 > final-gate.rc
