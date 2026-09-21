#!/usr/bin/env bash
set -euo pipefail

# Colors
GREEN='\033[0;32m'
BLUE='\033[0;34m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
NC='\033[0m'

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

CURRENT_VERSION=$(grep '^version = ' Cargo.toml | head -1 | cut -d '"' -f 2)

if [[ -z "$CURRENT_VERSION" ]]; then
    echo -e "${RED}Ошибка: не удалось определить текущую версию из Cargo.toml${NC}" >&2
    exit 1
fi

TARGET_VERSION=""
NO_BUMP=false
BUMP_ONLY=false

while [[ $# -gt 0 ]]; do
    case "$1" in
        -h|--help)
            echo -e "${BLUE}Скрипт сборки релизов JMCC${NC}"
            echo ""
            echo "Использование:"
            echo "  $0                   Инкремент версии (+0.1.0: ${CURRENT_VERSION} -> ...) и сборка релиза"
            echo "  $0 <версия>          Указать версию вручную (например: $0 1.1.1) и собрать"
            echo "  $0 -v, --version <v> Указать версию вручную (например: $0 -v 1.1.1) и собрать"
            echo "  $0 --no-bump         Собрать релиз с текущей версией (${CURRENT_VERSION}) без изменений"
            echo "  $0 --bump-only [v]   Только обновить версию во всех файлах проекта без сборки"
            exit 0
            ;;
        --no-bump|--keep)
            NO_BUMP=true
            shift
            ;;
        --bump-only)
            BUMP_ONLY=true
            shift
            ;;
        -v|--version)
            TARGET_VERSION="${2:-}"
            if [[ -z "$TARGET_VERSION" ]]; then
                echo -e "${RED}Ошибка: укажите версию после флага $1${NC}" >&2
                exit 1
            fi
            shift 2
            ;;
        *)
            if [[ -z "$TARGET_VERSION" && ! "$1" =~ ^- ]]; then
                TARGET_VERSION="$1"
                shift
            else
                echo -e "${RED}Ошибка: неизвестный аргумент '$1'${NC}" >&2
                exit 1
            fi
            ;;
    esac
done

if [[ "$NO_BUMP" == true ]]; then
    TARGET_VERSION="$CURRENT_VERSION"
elif [[ -z "$TARGET_VERSION" ]]; then
    # По умолчанию: инкремент minor версии (+0.1.0)
    IFS='.' read -r MAJOR MINOR PATCH <<< "$CURRENT_VERSION"
    NEW_MINOR=$((MINOR + 1))
    TARGET_VERSION="${MAJOR}.${NEW_MINOR}.0"
else
    # Очистка префикса 'v' (например, v1.1.1 -> 1.1.1)
    TARGET_VERSION="${TARGET_VERSION#v}"
fi

# Проверка формата SemVer
if [[ ! "$TARGET_VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[a-zA-Z0-9.]+)?$ ]]; then
    echo -e "${RED}Ошибка: неверный формат версии '${TARGET_VERSION}'. Ожидается SemVer (например: 0.2.0 или 1.1.1)${NC}" >&2
    exit 1
fi

if [[ "$TARGET_VERSION" != "$CURRENT_VERSION" ]]; then
    echo -e "${YELLOW}=== Обновление версии: ${CURRENT_VERSION} -> ${TARGET_VERSION} ===${NC}"

    # 1. Cargo.toml ([workspace.package])
    sed -i "s/^version = \"${CURRENT_VERSION}\"/version = \"${TARGET_VERSION}\"/" Cargo.toml
    echo -e "  ✓ Cargo.toml"

    # 2. VS Code extension package.json
    if [[ -f "jmc-analyzer/vscode/package.json" ]]; then
        sed -i "s/\"version\": \"${CURRENT_VERSION}\"/\"version\": \"${TARGET_VERSION}\"/" jmc-analyzer/vscode/package.json
        echo -e "  ✓ jmc-analyzer/vscode/package.json"
    fi

    # 3. Документация расширения VS Code
    for doc in jmc-analyzer/vscode/README.md jmc-analyzer/vscode/README_RU.md; do
        if [[ -f "$doc" ]]; then
            sed -i "s/justcode-lang-${CURRENT_VERSION}\.vsix/justcode-lang-${TARGET_VERSION}.vsix/g" "$doc"
            echo -e "  ✓ $doc"
        fi
    done

    # 4. Синхронизация Cargo.lock
    cargo generate-lockfile --offline
    echo -e "  ✓ Cargo.lock"
    echo -e "${GREEN}Версия успешно обновлена во всех файлах проекта: v${TARGET_VERSION}${NC}\n"
else
    echo -e "${BLUE}Версия не изменяется: ${TARGET_VERSION}${NC}\n"
fi

VERSION="$TARGET_VERSION"

if [[ "$BUMP_ONLY" == true ]]; then
    echo -e "${GREEN}Файлы успешно обновлены до версии v${VERSION}. Сборка пропущена (--bump-only).${NC}"
    exit 0
fi

OUT_DIR="$ROOT_DIR/target/releases"
mkdir -p "$OUT_DIR"
rm -rf "${OUT_DIR:?}"/*

echo -e "${BLUE}=== Сборка релиза JMCC v${VERSION} ===${NC}"
echo -e "Выходная директория: ${OUT_DIR}"

echo -e "\n${BLUE}[1/4] Сборка Linux binaries (x86_64-unknown-linux-gnu)...${NC}"
cargo build --release -p jmcc -p jmcmock -p jmc-analyzer

cp "$ROOT_DIR/target/release/jmcc" "$OUT_DIR/jmcc-linux-x64"
cp "$ROOT_DIR/target/release/jmcmock" "$OUT_DIR/jmcmock-linux-x64"
cp "$ROOT_DIR/target/release/jmc-analyzer" "$OUT_DIR/jmc-analyzer-linux-x64"

echo -e "Создание архива jmcc-v${VERSION}-linux-x64.tar.gz..."
tar -czf "$OUT_DIR/jmcc-v${VERSION}-linux-x64.tar.gz" -C "$ROOT_DIR/target/release" jmcc jmcmock jmc-analyzer

echo -e "\n${BLUE}[2/4] Сборка Windows MSVC binaries (x86_64-pc-windows-msvc)...${NC}"
cargo xwin build --release --target x86_64-pc-windows-msvc -p jmcc -p jmcmock -p jmc-analyzer

WIN_BIN_DIR="$ROOT_DIR/target/x86_64-pc-windows-msvc/release"
cp "$WIN_BIN_DIR/jmcc.exe" "$OUT_DIR/jmcc-windows-x64.exe"
cp "$WIN_BIN_DIR/jmcmock.exe" "$OUT_DIR/jmcmock-windows-x64.exe"
cp "$WIN_BIN_DIR/jmc-analyzer.exe" "$OUT_DIR/jmc-analyzer-windows-x64.exe"

echo -e "Создание архива jmcc-v${VERSION}-windows-x64.zip..."
(cd "$WIN_BIN_DIR" && zip -q -9 "$OUT_DIR/jmcc-v${VERSION}-windows-x64.zip" jmcc.exe jmcmock.exe jmc-analyzer.exe)

echo -e "\n${BLUE}[3/4] Сборка расширения VS Code (.vsix)...${NC}"
cargo run -p jmc-analyzer -- pack --output "$OUT_DIR/justcode-lang-${VERSION}.vsix"

echo -e "\n${BLUE}[4/4] Генерация контрольных сумм SHA256...${NC}"
cd "$OUT_DIR"
sha256sum \
    jmc-analyzer-linux-x64 \
    jmc-analyzer-windows-x64.exe \
    jmcc-linux-x64 \
    jmcc-v${VERSION}-linux-x64.tar.gz \
    jmcc-v${VERSION}-windows-x64.zip \
    jmcc-windows-x64.exe \
    jmcmock-linux-x64 \
    jmcmock-windows-x64.exe \
    justcode-lang-${VERSION}.vsix > sha256sums.txt

cat sha256sums.txt

echo -e "\n${GREEN}=== Все артефакты релиза v${VERSION} успешно собраны в target/releases/ ===${NC}"
