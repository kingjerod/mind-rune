#!/usr/bin/env bash
# Stand-in for the `claude` program in tests. Behaviour is chosen by $1.
case "$1" in
  env)
    env
    exit 0
    ;;
  big)
    head -c 5242880 /dev/zero | tr '\0' x
    echo
    exit 0
    ;;
  badutf8)
    printf '\xff\xfe ok\nsecond\n'
    exit 0
    ;;
  crash)
    echo before
    echo boom >&2
    exit 3
    ;;
  *)
    while IFS= read -r line; do
      echo "got:$line"
    done
    exit 0
    ;;
esac
