#!/bin/bash
set -e
cd /root/lanes/perry-coldextract
flock /root/MEASURE.lock bash -c '
 set -e
 echo "perry-coldextract/final micros+upm $(date -u +%FT%TZ)" > /root/MEASURE.holder
 taskset -c 56-63 setarch -R python3 measure_lane.py micro cycles > micro-cycles.log 2>&1
 echo 0 > micro-cycles.rc
 taskset -c 56-63 setarch -R python3 measure_lane.py upm cycles > upm-cycles.log 2>&1
 echo 0 > upm-cycles.rc
 ROW_PREFIX=locked-micro taskset -c 56-63 setarch -R python3 measure_lane.py micro gc > micro-locked-gc.log 2>&1
 echo 0 > micro-locked-gc.rc
 ROW_PREFIX=locked-upm taskset -c 56-63 setarch -R python3 measure_lane.py upm gc > upm-locked-gc.log 2>&1
 echo 0 > upm-locked-gc.rc
'
echo 0 > early-final-cycles.rc
