#!/bin/bash
# DeepSeek V3.1 con Claude Code

# Cargar variables de .env si existe
if [ -f ".env" ]; then
    set -a
    source .env
    set +a
fi

export ANTHROPIC_BASE_URL="https://api.deepseek.com/anthropic"
export ANTHROPIC_AUTH_TOKEN="${DEEPSEEK_API_KEY:?DEEPSEEK_API_KEY no está definida. Agrégala a tu .env o exportala.}"
export ANTHROPIC_MODEL="deepseek-v4-pro"
export ANTHROPIC_SMALL_FAST_MODEL="deepseek-v4-pro"

# Lanzar Claude Code
claude "$@"
