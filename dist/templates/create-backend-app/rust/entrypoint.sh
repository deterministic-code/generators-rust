#!/usr/bin/env sh
set -eu

# === BEGIN MIGRATE_HOOK — see PATCH_PLAN in create-migrate-scripts.mjs ===
# === END MIGRATE_HOOK ===

exec "$@"
