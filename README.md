# rawmakase-apple-photos

Importa una fototeca de Fotos (macOS) en [RAWmakase](https://github.com/pch/rawmakase): originales,
versiones editadas, álbumes y carpetas, favoritos, palabras clave, personas, descripciones, lugar y
fecha tal como los muestra Fotos. La fototeca nunca se modifica.

Es un prototipo. Probado en macOS 27.0.1 (Apple Silicon) con RAWmakase 0.7.2.

## Qué hay

| Carpeta | Qué es |
| --- | --- |
| `exportar/` | App auxiliar en Swift. Lee Fotos con PhotoKit y, en solo lectura, su base de datos; copia todo a una carpeta con sidecars XMP y un manifiesto. |
| `extension/` | Extensión de RAWmakase en Rust (`api = 3`). Lanza la app auxiliar, añade las fotos al catálogo abierto y crea los álbumes como colecciones. |
| `crear_colecciones.py` | Alternativa sin extensión: crea las colecciones por el control externo a partir del manifiesto. |
| `parches/` | Cambio propuesto para RAWmakase: un comando de control `stack`. Sin enviar. |
| `viabilidad/` | La primera prueba de acceso a Fotos con PhotoKit. |
| `PROPUESTA.md` | Borrador (en inglés) de la propuesta para el autor de RAWmakase. Sin enviar. |

## Compilar

Hacen falta las herramientas de línea de comandos de Xcode, Rust y `cargo rawmakase`
(`cargo install --locked --git https://github.com/pch/rawmakase --tag v0.7.2 cargo-rawmakase`).

```sh
sh compilar.sh                                   # FotosARawmakase.app (firma local)
mkdir -p extension/helper && cp -R FotosARawmakase.app extension/helper/
cd extension
cargo test
cargo rawmakase build
cargo rawmakase package \
  --include helper/FotosARawmakase.app/Contents/Info.plist \
  --include helper/FotosARawmakase.app/Contents/_CodeSignature/CodeResources \
  --include helper/FotosARawmakase.app/Contents/MacOS/fotos-a-rawmakase
```

El paquete queda en `extension/target/rmext/`. Se instala con
`rawmakase extensions install <archivo.rmext>`.

## Usar

En RAWmakase, Preferences > Extensions > Apple Photos Import:

1. Elegir la carpeta de destino ("Copy photos to").
2. Activar "Apple Photos" bajo Photo sources para el catálogo abierto.
3. Activar el control externo en Preferences, para que se creen los álbumes.
4. Lanzar "Import from Apple Photos".

La primera vez macOS pide acceso a Fotos para la app auxiliar. Se puede repetir: no copia nada dos veces.

## Límites conocidos

- Las ediciones de Fotos no se convierten en ajustes editables. Se copia la versión editada junto al
  original y se guardan los datos del ajuste de Apple, sin leer.
- Original y versión editada no quedan apilados: RAWmakase 0.7.2 no tiene comando para ello (véase
  `parches/`).
- Solo la fototeca del sistema. Los vídeos se copian si se pide, pero RAWmakase no los muestra.
- La lectura de la base de datos de Fotos solo está comprobada en macOS 27. El manifiesto anota la
  versión y las columnas usadas.
- La app auxiliar va firmada solo localmente: en otro Mac, macOS puede bloquearla.

## Estado

Nada se ha enviado al autor de RAWmakase. El cambio de `parches/` lo ha revisado un agente de Claude
(Opus), que según las reglas de ese proyecto es un sustituto débil: falta la revisión de un modelo de
otro proveedor y el `make check` completo.
