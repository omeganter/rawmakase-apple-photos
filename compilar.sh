#!/bin/sh
# Compila el exportador como FotosARawmakase.app (firma local). Necesita las herramientas de línea de comandos de Xcode.
set -eu
cd "$(dirname "$0")"
APP=FotosARawmakase.app
mkdir -p "$APP/Contents/MacOS"
swiftc -O -swift-version 5 exportar/main.swift -o "$APP/Contents/MacOS/fotos-a-rawmakase"
cp exportar/Info.plist "$APP/Contents/Info.plist"
codesign --force -s - "$APP"
echo "Hecho: $PWD/$APP"
