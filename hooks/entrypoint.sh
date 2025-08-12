#!/bin/sh

exec /usr/local/bin/ldactl --exec /hooks/${LDACTL_ACTION}.sh "$@"