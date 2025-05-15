#!/bin/sh
cd /usr/src || exit 1
set --
[ "$WRITABLE" = "1" ] && set -- "$@" --writable
[ -n "$UNLOADTIME" ] && set -- "$@" --unload-time "$UNLOADTIME"
[ "$DEBUG" = "1" ] && set -- "$@" --debug
sudo -u user /usr/bin/textsurf --bind 0.0.0.0:8080 --basedir=/data "$@" || sleep 5 #sleep is a safeguard against continuous restarts in case of failure
