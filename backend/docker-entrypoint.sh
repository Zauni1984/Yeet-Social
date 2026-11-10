#!/bin/sh
# Container entrypoint: the image runs the service as the unprivileged
# user `yeet` (audit I-M7). The persistent volumes (uploads, private
# vault) were created by earlier root-run containers, so on start-up we
# hand them to `yeet` once and then drop privileges for good. A
# container started with `--user` already runs unprivileged and skips
# the chown.
set -eu
if [ "$(id -u)" = "0" ]; then
  for d in "${UPLOADS_DIR:-/app/uploads}" "${PRIVATE_DIR:-/app/private}"; do
    mkdir -p "$d"
    # Only touch files that are not already ours: cheap on every restart.
    find "$d" \( ! -user yeet -o ! -group yeet \) -exec chown yeet:yeet {} + 2>/dev/null || true
  done
  exec setpriv --reuid=yeet --regid=yeet --init-groups "$@"
fi
exec "$@"
