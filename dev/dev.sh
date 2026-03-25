#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"

usage() {
    cat <<'EOF'
Usage: dev.sh <command> [args]

Commands:
  build       Build syfrah binary (cargo build)
  up          Start containers (build image if needed)
  down        Stop and remove containers
  restart     Rebuild binary + restart containers
  exec <node> <cmd...>   Run a command on a node (node1 or node2)
  n1 <cmd...>            Shortcut for: exec node1 <cmd...>
  n2 <cmd...>            Shortcut for: exec node2 <cmd...>
  logs [node]            Show container logs
  status      Show container and WireGuard status on both nodes
  shell <node>           Open a shell on a node
  clean       Stop containers and remove images

Examples:
  ./dev.sh build
  ./dev.sh up
  ./dev.sh n1 syfrah fabric init
  ./dev.sh n2 syfrah fabric join 172.28.0.2:9900
  ./dev.sh n1 syfrah fabric peers
  ./dev.sh status
  ./dev.sh shell node1
EOF
}

cmd_build() {
    echo "==> Building syfrah binary..."
    (cd "$PROJECT_ROOT" && cargo build)
    echo "==> Build complete."
}

cmd_up() {
    # Ensure wireguard kernel module is loaded on the host
    if ! lsmod | grep -q wireguard; then
        echo "==> Loading wireguard kernel module..."
        sudo modprobe wireguard || {
            echo "ERROR: Could not load wireguard module. Install it first:"
            echo "  sudo apt install wireguard"
            exit 1
        }
    fi

    # Check binary exists
    if [ ! -f "$PROJECT_ROOT/target/debug/syfrah" ]; then
        echo "==> Binary not found, building first..."
        cmd_build
    fi

    echo "==> Starting containers..."
    docker compose -f "$SCRIPT_DIR/docker-compose.yml" up -d --build
    echo "==> Containers ready."
    echo "    node1: docker exec syfrah-node1 ..."
    echo "    node2: docker exec syfrah-node2 ..."
}

cmd_down() {
    echo "==> Stopping containers..."
    docker compose -f "$SCRIPT_DIR/docker-compose.yml" down
}

cmd_restart() {
    cmd_build
    cmd_down
    cmd_up
}

cmd_exec() {
    local node="$1"; shift
    docker exec -it "syfrah-${node}" "$@"
}

cmd_logs() {
    local node="${1:-}"
    if [ -n "$node" ]; then
        docker compose -f "$SCRIPT_DIR/docker-compose.yml" logs -f "$node"
    else
        docker compose -f "$SCRIPT_DIR/docker-compose.yml" logs -f
    fi
}

cmd_status() {
    for node in node1 node2; do
        echo "=== $node ==="
        docker exec "syfrah-${node}" sh -c '
            echo "-- IP addresses --"
            ip -brief addr
            echo ""
            echo "-- WireGuard --"
            wg show 2>/dev/null || echo "(no wireguard interface)"
            echo ""
            echo "-- syfrah status --"
            syfrah fabric status 2>/dev/null || echo "(daemon not running)"
        '
        echo ""
    done
}

cmd_shell() {
    local node="$1"
    docker exec -it "syfrah-${node}" /bin/bash
}

cmd_clean() {
    cmd_down
    docker compose -f "$SCRIPT_DIR/docker-compose.yml" down --rmi local
}

# --- Main ---
if [ $# -eq 0 ]; then
    usage
    exit 1
fi

command="$1"; shift

case "$command" in
    build)   cmd_build ;;
    up)      cmd_up ;;
    down)    cmd_down ;;
    restart) cmd_restart ;;
    exec)    cmd_exec "$@" ;;
    n1)      cmd_exec "node1" "$@" ;;
    n2)      cmd_exec "node2" "$@" ;;
    logs)    cmd_logs "${1:-}" ;;
    status)  cmd_status ;;
    shell)   cmd_shell "$1" ;;
    clean)   cmd_clean ;;
    -h|--help|help) usage ;;
    *)
        echo "Unknown command: $command"
        usage
        exit 1
        ;;
esac
