#!/bin/bash
set -e
cd /root/lanes/perry-coldextract
NAMES=hello,tsc,zod,qs_parse,qs_stringify,commander taskset -c 0-55 setarch -R python3 measure_lane.py programs inst > programs-early-inst.log 2>&1
echo 0 > early-programs-inst.rc
