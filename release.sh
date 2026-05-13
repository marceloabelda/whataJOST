#!/bin/bash
# release.sh - Bump de versión, actualiza dependencias y pushea el tag.
# El build y la publicación del release los hace GitHub CI.
# Uso: ./release.sh 1.2.0

set -e

if [ -z "$1" ]; then
    echo "Uso: $0 <nueva_version>"
    exit 1
fi

VERSION="$1"
BRANCH=$(git rev-parse --abbrev-ref HEAD)

if [ "$BRANCH" != "main" ]; then
    echo "⚠️  No estás en la rama main. Cambiá a main antes de continuar."
    exit 1
fi

if ! git diff-index --quiet HEAD --; then
    echo "⚠️  Hay cambios sin commitear. Hacé commit o stash antes de continuar."
    exit 1
fi

echo "🔖 Preparando release v${VERSION}..."

# Actualizar versión en los tres archivos
sed -i "s/\"version\": \".*\"/\"version\": \"${VERSION}\"/" package.json
sed -i "s/^version = \".*\"$/version = \"${VERSION}\"/" src-tauri/Cargo.toml
sed -i "s/\"version\": \".*\"/\"version\": \"${VERSION}\"/" src-tauri/tauri.conf.json

echo "📦 Actualizando dependencias..."
cargo update --manifest-path src-tauri/Cargo.toml
pnpm update

# Commitear todo: versión + lock files actualizados
git add .
git commit -m "Release v${VERSION}"

# Borrar tag local si ya existía de un intento fallido anterior
git tag -d "v${VERSION}" 2>/dev/null || true
git tag "v${VERSION}" -m "Release v${VERSION}"

echo "📤 Pusheando a GitHub (el CI buildea y publica el release)..."
git push origin main "v${VERSION}"

echo ""
echo "✅ Tag v${VERSION} pusheado. Seguí el build en:"
echo "   https://github.com/marceloabelda/whataJOST/actions"
