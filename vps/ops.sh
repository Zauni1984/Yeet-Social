#!/bin/bash
# Manual server tasks, called by .github/workflows/ops.yml:
#   ops.sh <task> [arg]
# Never prints secrets. Paths follow the account it runs as (root: /root/…,
# deploy: /opt/yeet/…), like vps/cd-deploy.sh.
set -euo pipefail
TASK="${1:-status}"; ARG="${2:-}"
RAW="https://raw.githubusercontent.com/Zauni1984/Yeet-Social/${OPS_SHA:-main}"
if [ "$(id -u)" = "0" ]; then APP=/root/yeet-social; else APP=/opt/yeet/app; fi
# After the switch to the deploy account /opt/yeet/app is authoritative, even
# for a task that had to run as root.
[ -f /opt/yeet/app/.last_deploy_user ] && [ "$(cat /opt/yeet/app/.last_deploy_user)" = "deploy" ] && APP=/opt/yeet/app
ENVF="$APP/.env"
NOTE_VOL=yeet-social_note_data
NOTE_IMG=yeet/note-node:1.1.0.0
LOG=/tmp/yeet-note-build.log

env_has(){ grep -q "^$1=" "$ENVF" 2>/dev/null; }
env_set(){ if env_has "$1"; then sed -i "s|^$1=.*|$1=$2|" "$ENVF"; else echo "$1=$2" >> "$ENVF"; fi; }
env_get(){ grep "^$1=" "$ENVF" 2>/dev/null | head -1 | cut -d= -f2-; }
cli(){ docker exec yeet-note notecoin-cli -datadir=/data "$@"; }

case "$TASK" in
  status)
    echo "user: $(id -un) · app dir: $APP · last deploy by: $(cat "$APP/.last_deploy_user" 2>/dev/null || echo '?')"
    echo "deploy account: $(id deploy 2>/dev/null || echo 'absent')"
    echo "root has CD key: $(grep -c github-actions-deploy /root/.ssh/authorized_keys 2>/dev/null || echo '?')"
    df -h / | tail -1; free -m | sed -n 2p; nproc
    docker ps --format 'table {{.Names}}\t{{.Status}}\t{{.Image}}'
    curl -s http://127.0.0.1:8080/api/v1/health || true; echo
    ;;

  setup-deploy-user)
    [ "$(id -u)" = "0" ] || { echo "needs root"; exit 1; }
    curl -fsSL "$RAW/vps/setup-deploy-user.sh" -o /tmp/setup-deploy-user.sh
    bash /tmp/setup-deploy-user.sh
    echo "--- verification"; id deploy; ls -la /opt/yeet /opt/yeet/app | head -20
    ;;

  finalize-deploy-user)
    # Only after a deploy really ran as `deploy`: drop the CD key from root.
    # PermitRootLogin is deliberately left alone — the operator's own root
    # access (password / personal key / provider console) must not be cut
    # off from here.
    [ "$(id -u)" = "0" ] || { echo "needs root"; exit 1; }
    [ "$(cat /opt/yeet/app/.last_deploy_user 2>/dev/null)" = "deploy" ] || { echo "REFUSED: the last deploy did not run as 'deploy' yet"; exit 1; }
    cp -a /root/.ssh/authorized_keys "/root/.ssh/authorized_keys.bak-$(date +%Y%m%d-%H%M%S)"
    sed -i '/github-actions-deploy/d' /root/.ssh/authorized_keys
    echo "removed CD key from root; remaining root keys: $(grep -c . /root/.ssh/authorized_keys || true)"
    ;;

  note-install)
    # Builds the Note daemon image from the pinned source commit (in the
    # background, low priority) and wires the service into the stack.
    [ -f "$ENVF" ] || { echo "no .env at $ENVF"; exit 1; }
    env_has NOTE_RPC_USER || env_set NOTE_RPC_USER yeetswap
    env_has NOTE_RPC_PASS || env_set NOTE_RPC_PASS "$(openssl rand -hex 32)"
    env_has NOTE_RPC_URL  || env_set NOTE_RPC_URL  "http://yeet-note:9332"
    mkdir -p /tmp/yeet-note-build && cd /tmp/yeet-note-build
    for f in Dockerfile entrypoint.sh; do curl -fsSL "$RAW/vps/note-node/$f" -o "$f"; done
    cat > build.sh <<'BUILD'
#!/bin/bash
set -e
cd /tmp/yeet-note-build
echo "[note] build started $(date)"
nice -n 15 docker build --cpu-shares 256 -t yeet/note-node:1.1.0.0 .
APP="$1"; cd "$APP"
grep -q '^COMPOSE_PROFILES=' .env && sed -i 's|^COMPOSE_PROFILES=.*|COMPOSE_PROFILES=note|' .env || echo 'COMPOSE_PROFILES=note' >> .env
docker compose up -d note-node
echo "[note] BUILD_AND_START_OK $(date)"
BUILD
    chmod +x build.sh
    if docker image inspect "$NOTE_IMG" >/dev/null 2>&1; then
      echo "image exists — (re)starting service"; cd "$APP"; env_set COMPOSE_PROFILES note; docker compose up -d note-node
    else
      nohup ./build.sh "$APP" > "$LOG" 2>&1 &
      echo "build started in the background (≈20–40 min). Check with task note-status."
    fi
    ;;

  note-status)
    echo "--- build log tail"; tail -n 12 "$LOG" 2>/dev/null || echo "(no build log)"
    echo "--- container"; docker ps -a --filter name=yeet-note --format '{{.Names}}\t{{.Status}}'
    echo "--- chain"; cli getblockchaininfo 2>&1 | grep -E '"chain"|"blocks"|"headers"|"verificationprogress"|"initialblockdownload"|"size_on_disk"|error' || true
    echo "--- peers: $(cli getconnectioncount 2>&1 | tail -1)"
    echo "--- disk"; df -h / | tail -1; docker run --rm -v $NOTE_VOL:/d alpine du -sh /d 2>/dev/null || true
    echo "--- daemon log tail"; docker logs --tail 15 yeet-note 2>&1 | cut -c1-200 || true
    ;;

  note-import)
    # Restore a chain backup when the node cannot sync from peers. ARG is a
    # URL or an absolute path on this server to: a .tar/.tar.gz/.tgz/.zip
    # containing blocks/ and chainstate/ (a copied datadir), or a
    # bootstrap.dat. The wallet in the volume is never touched.
    [ -n "$ARG" ] || { echo "usage: note-import <url-or-absolute-path>"; exit 1; }
    docker stop yeet-note 2>/dev/null || true
    SRCMOUNT=""; SRC="$ARG"
    case "$ARG" in /*) [ -f "$ARG" ] || { echo "no such file: $ARG"; exit 1; }; SRCMOUNT="-v $(dirname "$ARG"):/in:ro"; SRC="/in/$(basename "$ARG")";; esac
    docker run --rm -u 0 -v $NOTE_VOL:/data $SRCMOUNT --entrypoint /bin/bash "$NOTE_IMG" -c '
      set -e; SRC="$0"; W=/data/.import; rm -rf "$W"; mkdir -p "$W"; cd "$W"
      case "$SRC" in http*) echo "downloading…"; curl -fL --retry 3 -o payload "$SRC";; *) ln -s "$SRC" payload;; esac
      NAME=$(echo "$SRC" | tr A-Z a-z)
      case "$NAME" in
        *bootstrap.dat) cp payload /data/bootstrap.dat; echo "bootstrap.dat placed (imported on next start)";;
        *.zip) unzip -q payload -d x;;
        *.tar.gz|*.tgz) mkdir x; tar -xzf payload -C x;;
        *.tar) mkdir x; tar -xf payload -C x;;
        *) echo "unknown backup format: $SRC"; exit 1;;
      esac
      if [ -d x ]; then
        B=$(find x -type d -name blocks | head -1); C=$(find x -type d -name chainstate | head -1); BS=$(find x -type f -name bootstrap.dat | head -1)
        if [ -n "$B" ] && [ -n "$C" ]; then
          rm -rf /data/blocks.old /data/chainstate.old
          [ -d /data/blocks ] && mv /data/blocks /data/blocks.old; [ -d /data/chainstate ] && mv /data/chainstate /data/chainstate.old
          mv "$B" /data/blocks; mv "$C" /data/chainstate; echo "blocks + chainstate restored (previous kept as *.old)"
        elif [ -n "$BS" ]; then mv "$BS" /data/bootstrap.dat; echo "bootstrap.dat placed"
        else echo "archive has neither blocks/+chainstate/ nor bootstrap.dat"; exit 1; fi
      fi
      rm -rf "$W"; chown -R note:note /data' "$SRC"
    docker start yeet-note; sleep 8; docker logs --tail 8 yeet-note 2>&1 | cut -c1-200
    ;;

  note-restart) cd "$APP"; docker compose up -d --force-recreate note-node; sleep 5; docker logs --tail 8 yeet-note 2>&1 | cut -c1-200 ;;

  *) echo "unknown task: $TASK"; exit 1 ;;
esac
