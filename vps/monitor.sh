#!/bin/bash
APP_DIR="/opt/yeet"
cd $APP_DIR
case "${1:-status}" in
  status)
    echo "═══════════════════════════════════"
    echo "  Yeet — Container Status"
    echo "═══════════════════════════════════"
    docker compose ps
    echo ""
    df -h / | tail -1
    free -h | grep Mem
    uptime
    ;;
  logs)
    SERVICE="${2:-backend}"
    docker compose logs --tail=100 -f "$SERVICE"
    ;;
  restart)
    SERVICE="${2:-}"
    if [[ -z "$SERVICE" ]]; then
      docker compose restart
    else
      docker compose restart "$SERVICE"
    fi
    ;;
  backup)
    BACKUP_DIR="/opt/yeet-backups"
    mkdir -p "$BACKUP_DIR"
    TS=$(date +%Y%m%d_%H%M%S)
    docker compose exec -T postgres pg_dump -U yeet yeet | gzip > "$BACKUP_DIR/db_${TS}.sql.gz"
    find "$BACKUP_DIR" -name "*.sql.gz" -mtime +7 -delete
    echo "✅ Backup: $BACKUP_DIR/db_${TS}.sql.gz"
    ;;
  update)
    bash /opt/yeet/deploy.sh
    ;;
  *)
    echo "Verwendung: $0 {status|logs [service]|restart [service]|backup|update}"
    ;;
esac
