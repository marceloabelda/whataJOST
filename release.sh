#!/bin/bash
# release.sh - Publica una nueva versión en GitHub con soporte de auto-update
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

# Verificar clave privada del updater
KEY_FILE="src-tauri/updater_key"
if [ ! -f "$KEY_FILE" ]; then
    echo "⚠️  No se encontró la clave privada del updater en $KEY_FILE"
    echo "   Generala con: npx tauri signer generate -w src-tauri/updater_key --ci"
    exit 1
fi

# Verificar gh CLI
if ! command -v gh &> /dev/null; then
    echo "⚠️  gh CLI no encontrado. Instalalo para crear releases automáticamente."
    echo "   https://cli.github.com/"
    exit 1
fi

echo "🔖 Preparando release v${VERSION}..."

# Actualizar versión en los archivos necesarios
sed -i "s/\"version\": \".*\"/\"version\": \"${VERSION}\"/" package.json
sed -i "s/^version = \".*\"$/version = \"${VERSION}\"/" src-tauri/Cargo.toml
sed -i "s/\"version\": \".*\"/\"version\": \"${VERSION}\"/" src-tauri/tauri.conf.json

# Commit de los cambios de versión
git add .
git commit -m "Release v${VERSION}"

# Borrar tag local si ya existe (de un intento fallido anterior)
git tag -d "v${VERSION}" 2>/dev/null || true

# Crear tag
git tag "v${VERSION}" -m "Release v${VERSION}"

# Setear claves para firmar los bundles
export TAURI_SIGNING_PRIVATE_KEY="$(cat "$KEY_FILE")"
export TAURI_SIGNING_PRIVATE_KEY_PASSWORD=""

echo "🔨 Buildenado la app para Linux..."
# Limpiar builds anteriores para que no se cuelen bundles viejos en el latest.json
rm -rf src-tauri/target/release/bundle
npx tauri build 2>&1

echo "📤 Subiendo commit y tag a GitHub..."
git push origin main "v${VERSION}"

echo "📦 Creando manifiesto de actualización..."

# Directorio donde tauri deja los bundles
BUNDLE_DIR="src-tauri/target/release/bundle"

# Determinar arquitectura
ARCH=$(uname -m)
case "$ARCH" in
    x86_64)  TARGET_ARCH="x86_64" ;;
    aarch64) TARGET_ARCH="aarch64" ;;
    *)       echo "⚠️  Arquitectura no soportada: $ARCH"; exit 1 ;;
esac

# Determinar SO
case "$(uname -s)" in
    Linux)  TARGET_OS="linux" ;;
    Darwin) TARGET_OS="darwin" ;;
    *)      echo "⚠️  SO no soportado"; exit 1 ;;
esac

TARGET="${TARGET_OS}-${TARGET_ARCH}"
NOW=$(date -u +%Y-%m-%dT%H:%M:%SZ)
REPO_URL="https://github.com/marceloabelda/whataJOST"

# Construir el latest.json dinámicamente
echo "{" > latest.json
echo "  \"version\": \"v${VERSION}\"," >> latest.json
echo "  \"notes\": \"Release v${VERSION}\"," >> latest.json
echo "  \"pub_date\": \"${NOW}\"," >> latest.json
echo "  \"platforms\": {" >> latest.json

FIRST=true
for BUNDLE in "$BUNDLE_DIR"/deb/*.deb "$BUNDLE_DIR"/rpm/*.rpm "$BUNDLE_DIR"/app/*.AppImage "$BUNDLE_DIR"/app/*.tar.gz "$BUNDLE_DIR"/dmg/*.dmg "$BUNDLE_DIR"/msi/*.msi; do
    [ -f "$BUNDLE" ] || continue

    # Buscar archivo .sig correspondiente
    SIG_FILE="${BUNDLE}.sig"
    [ -f "$SIG_FILE" ] || continue

    SIGNATURE=$(cat "$SIG_FILE")
    FILENAME=$(basename "$BUNDLE")

    # Determinar el target a partir del nombre o directorio
    if [[ "$BUNDLE" == *"/deb/"* ]] || [[ "$BUNDLE" == *"/rpm/"* ]] || [[ "$BUNDLE" == *"/app/"* ]]; then
        BUNDLE_TARGET="$TARGET"
    elif [[ "$BUNDLE" == *"/dmg/"* ]]; then
        BUNDLE_TARGET="$TARGET"
    elif [[ "$BUNDLE" == *"/msi/"* ]]; then
        BUNDLE_TARGET="windows-x86_64"
    fi

    DOWNLOAD_URL="${REPO_URL}/releases/download/v${VERSION}/${FILENAME}"

    if [ "$FIRST" = true ]; then
        FIRST=false
    else
        echo "    ," >> latest.json
    fi

    echo "    \"${BUNDLE_TARGET}\": {" >> latest.json
    echo "      \"signature\": \"${SIGNATURE}\"," >> latest.json
    echo "      \"url\": \"${DOWNLOAD_URL}\"" >> latest.json
    echo "    }" >> latest.json
done

echo "  }" >> latest.json
echo "}" >> latest.json

echo "📄 Manifiesto latest.json:"
cat latest.json

echo ""
echo "🚀 Creando release en GitHub y subiendo archivos..."

# Crear release y subir assets
ASSETS=()
for BUNDLE in "$BUNDLE_DIR"/deb/*.deb "$BUNDLE_DIR"/rpm/*.rpm "$BUNDLE_DIR"/app/*.AppImage "$BUNDLE_DIR"/app/*.tar.gz "$BUNDLE_DIR"/dmg/*.dmg "$BUNDLE_DIR"/msi/*.msi; do
    [ -f "$BUNDLE" ] && ASSETS+=("$BUNDLE")
    [ -f "${BUNDLE}.sig" ] && ASSETS+=("${BUNDLE}.sig")
done
ASSETS+=("latest.json")

gh release create "v${VERSION}" "${ASSETS[@]}" \
    --title "v${VERSION}" \
    --notes "Release v${VERSION}" \
    --repo marceloabelda/whataJOST

echo "✅ Release v${VERSION} publicada con auto-update."
