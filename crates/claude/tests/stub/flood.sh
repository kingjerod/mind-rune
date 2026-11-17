#!/usr/bin/env bash
# Stand-in for the `claude` program in the test of a large input line. It waits until the file
# named by its first argument exists (the test makes it once the large line is queued; after
# 10 seconds it goes on anyway, so a broken test cannot hold it for ever), prints "ready", then
# 300 lines of 1,024 characters (more than a pipe holds) before it reads anything, and only then
# echoes the length of each input line.
marker=$1
for _ in $(seq 1000); do
  [ -e "$marker" ] && break
  sleep 0.01
done
echo ready
line=$(head -c 1024 /dev/zero | tr '\0' x)
for _ in $(seq 300); do
  echo "$line"
done
while IFS= read -r line; do
  echo "${#line}"
done
