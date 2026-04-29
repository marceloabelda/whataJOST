#!/bin/bash
# release.sh - Publica una nueva versión en GitHub
# Uso: ./release.sh 1.2.0

set -e

if [ -z "$1" ]; then
    echo "Uso: $0 <nueva_version>"
    exit 1
fi

VERSION="$1"
BRANCH=$(git rev-parse --abbrev-ref HEAD)

if [ "$BRANCH" != "main" ]; then
    echo "⚠️  No estás en la rama main. Cambiá a main o ajustá el script."
    exit 1
fi

# Verificar que no haya cambios sin commitear
if ! git diff-index --quiet HEAD --; then
    echo "⚠️  Hay cambios sin commitear. Hacé commit o stash antes de continuar."
    exit 1
fi

echo "🔖 Preparando release v${VERSION}..."

# Actualizar versión en los archivos necesarios
sed -i "s/\"version\": \".*\"/\"version\": \"${VERSION}\"/" package.json
sed -i "s/^version = \".*\"$/version = \"${VERSION}\"/" src-tauri/Cargo.toml
sed -i "s/\"version\": \".*\"/\"version\": \"${VERSION}\"/" src-tauri/tauri.conf.json

# Commit de los cambios de versión
git add . ; git commit -m "Agrego version nueva. v${VERSION}"

# Crear tag
git tag "v${VERSION}" -m "Release v${VERSION}"

# Pushear rama y tag
git push origin main


echo "🚀 Release v${VERSION} subido a GitHub. Podés crear la release en la interfaz web o con gh CLI."