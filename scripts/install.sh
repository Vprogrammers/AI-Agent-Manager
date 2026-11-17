#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DEST="${HOME}/.local/bin"
mkdir -p "$DEST"
cat > "$DEST/aam" <<EOF
#!/usr/bin/env bash
exec python3 "$ROOT/src/ai_agent_manager.py" "\$@"
EOF
chmod +x "$DEST/aam"
printf 'Installed %s/aam. Ensure ~/.local/bin is on PATH.\n' "$DEST"
