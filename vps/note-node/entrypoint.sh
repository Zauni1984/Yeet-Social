#!/bin/bash
# Writes notecoin.conf from the environment on every start (so a rotated RPC
# password in .env takes effect) and runs the daemon in the foreground.
set -euo pipefail
: "${NOTE_RPC_USER:?NOTE_RPC_USER not set}"; : "${NOTE_RPC_PASS:?NOTE_RPC_PASS not set}"
CONF=/data/notecoin.conf
umask 077
{
  echo "server=1"
  echo "listen=1"
  echo "printtoconsole=1"
  echo "rpcuser=${NOTE_RPC_USER}"
  echo "rpcpassword=${NOTE_RPC_PASS}"
  # RPC only inside the compose network; no port is published on the host.
  echo "rpcbind=0.0.0.0"
  echo "rpcallowip=172.16.0.0/12"
  echo "rpcallowip=127.0.0.1"
  echo "rpcport=9332"
  echo "dbcache=${NOTE_DBCACHE:-200}"
  echo "maxconnections=${NOTE_MAXCONN:-24}"
  # Extra peers when the network's seeds do not answer (comma separated).
  IFS=',' read -ra NODES <<< "${NOTE_ADDNODES:-}"
  for n in "${NODES[@]}"; do [ -n "$n" ] && echo "addnode=$n"; done
} > "$CONF"
# A bootstrap.dat dropped into the volume (ops task note-import) is imported
# by the daemon itself on start and renamed to bootstrap.dat.old.
exec notecoind -datadir=/data -conf="$CONF"
