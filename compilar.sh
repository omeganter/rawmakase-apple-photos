#!/bin/sh
# Compila el lector de Fotos como FotosARawmakase.app (firma local). Necesita Rust y las
# herramientas de línea de comandos de Xcode (por `codesign` y las bibliotecas del sistema).
set -eu
cd "$(dirname "$0")"
APP=FotosARawmakase.app
(cd exportar && cargo build --release --locked)
mkdir -p "$APP/Contents/MacOS"
cp exportar/target/release/fotos-a-rawmakase "$APP/Contents/MacOS/fotos-a-rawmakase"
cp exportar/Info.plist "$APP/Contents/Info.plist"
codesign --force -s - "$APP"
echo "Hecho: $PWD/$APP"
