#!/usr/bin/env python3
"""Crea en RAWmakase las carpetas y álbumes de Fotos a partir de _fotos/manifiesto.json.

Antes: abre RAWmakase, añade al catálogo la carpeta exportada (Añadir carpeta) y deja la app abierta
con el control externo activado. Este script no toca ningún archivo de fotos: solo crea colecciones.

    python3 crear_colecciones.py /ruta/destino [--control /ruta/control.json] [--simular]

Habla el protocolo JSON de RAWmakase (docs/automation.md, versión 1): `photos`, `collections`,
`create_collection` y `add_to_collection`. Probado con RAWmakase 0.7.2.

El manifiesto dice con qué versión de macOS se exportó y si la lectura de la base de datos de Fotos
estaba comprobada para ella (bloque "origen"); aquí se muestra y se avisa si no lo estaba.
"""
import argparse
import json
import os
import socket
import sys
import unicodedata
import uuid

MANIFIESTO_MAXIMO = 2  # versión más reciente del manifiesto que este script entiende
LOTE = 400  # ids por petición; el límite de una petición es 64 KiB

DATOS = [
    "~/Library/Application Support/RAWmakase",
    "~/Library/Application Support/rawmakase",
    "~/.local/share/rawmakase",
]


def buscar_control(ruta):
    candidatos = [ruta] if ruta else [os.path.join(os.path.expanduser(d), "control.json") for d in DATOS]
    for c in candidatos:
        if c and os.path.isfile(c):
            with open(c, encoding="utf-8") as f:
                return json.load(f)
    sys.exit("No encuentro control.json. ¿Está RAWmakase abierto con el control externo activado? "
             "Indica su ruta con --control.")


class Control:
    def __init__(self, conexion):
        self.puerto, self.token = conexion["port"], conexion["token"]

    def pedir(self, cmd, **campos):
        peticion = {"protocol": 1, "request_id": uuid.uuid4().hex, "token": self.token, "cmd": cmd, **campos}
        with socket.create_connection(("127.0.0.1", self.puerto), timeout=60) as s:
            s.sendall((json.dumps(peticion) + "\n").encode())
            datos = b""
            while not datos.endswith(b"\n"):
                trozo = s.recv(65536)
                if not trozo:
                    break
                datos += trozo
        respuesta = json.loads(datos)
        if not respuesta.get("ok"):
            raise RuntimeError(f"{cmd}: {respuesta.get('code')}: {respuesta.get('error')}")
        return respuesta.get("result", {})


def clave(ruta):
    """Ruta comparable: absoluta, normalizada (NFC) y sin distinguir mayúsculas."""
    return unicodedata.normalize("NFC", os.path.realpath(ruta)).casefold()


def fotos_del_catalogo(control):
    """Devuelve {ruta normalizada: id} de todas las fotos del catálogo abierto."""
    por_ruta, desde = {}, 0
    while True:
        r = control.pedir("photos", offset=desde, limit=500)
        lista = r.get("photos") or r.get("entries") or []
        if not lista:
            break
        if "path" not in lista[0] or "id" not in lista[0]:
            sys.exit(f"Formato inesperado de `photos`; primera entrada: {json.dumps(lista[0])[:400]}")
        for f in lista:
            por_ruta[clave(f["path"])] = f["id"]
        desde += len(lista)
        if desde >= r.get("total", desde):
            break
    return por_ruta


def main():
    p = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    p.add_argument("destino", help="carpeta exportada (la que contiene _fotos/manifiesto.json)")
    p.add_argument("--control", help="ruta de control.json de RAWmakase")
    p.add_argument("--simular", action="store_true", help="solo dice lo que haría")
    a = p.parse_args()

    with open(os.path.join(a.destino, "_fotos", "manifiesto.json"), encoding="utf-8") as f:
        manifiesto = json.load(f)
    version = manifiesto.get("version", 1)
    if version > MANIFIESTO_MAXIMO:
        sys.exit(f"El manifiesto es de la versión {version} y este script entiende hasta la {MANIFIESTO_MAXIMO}. "
                 "Actualiza crear_colecciones.py.")
    origen = manifiesto.get("origen", {})
    print(f"Manifiesto v{version} · {manifiesto.get('herramienta', 'fotos-a-rawmakase 0.1')} · "
          f"macOS {origen.get('macos', 'desconocido')} · metadatos: {origen.get('metadatos', 'solo favoritos')}")
    if origen and not origen.get("macos_comprobado", False):
        print("Aviso: la lectura de la base de datos de Fotos no estaba comprobada para esa versión de macOS. "
              "Revisa unas cuantas fotos con palabras clave o personas antes de fiarte.")
    if origen.get("esquema_fotos", {}).get("no_encontrado"):
        print("Aviso: no se pudo leer de Fotos: " + ", ".join(origen["esquema_fotos"]["no_encontrado"]))
    control = Control(buscar_control(a.control))

    por_ruta = fotos_del_catalogo(control)
    print(f"Fotos en el catálogo: {len(por_ruta)}")

    # id de Fotos -> ids de RAWmakase (original, RAW y versión editada)
    ids = {}
    for foto in manifiesto["fotos"]:
        encontrados = []
        for campo in ("original", "raw", "editada"):
            rel = foto.get(campo)
            if rel and clave(os.path.join(a.destino, rel)) in por_ruta:
                encontrados.append(por_ruta[clave(os.path.join(a.destino, rel))])
        ids[foto["id"]] = encontrados

    existentes = {(c.get("parent_id"), c["name"]): c["id"] for c in control.pedir("collections").get("collections", [])}
    creadas = {}  # id de Fotos -> id de RAWmakase
    sin_foto = 0

    for col in manifiesto["colecciones"]:  # las carpetas van antes que su contenido
        padre = creadas.get(col.get("padre")) if col.get("padre") else None
        nombre = col["nombre"]
        if col["tipo"] == "carpeta":
            if (padre, nombre) in existentes:
                creadas[col["id"]] = existentes[(padre, nombre)]
            elif a.simular:
                print(f"[carpeta] {nombre}")
                creadas[col["id"]] = f"sim-{col['id']}"
            else:
                campos = {"name": nombre, "kind": "set"}
                if padre is not None:
                    campos["parent_id"] = padre
                creadas[col["id"]] = control.pedir("create_collection", **campos)["created"]["id"]
            continue

        fotos = []
        for fid in col.get("fotos", []):
            if not ids.get(fid):
                sin_foto += 1
            fotos.extend(ids.get(fid, []))
        print(f"[álbum] {nombre}: {len(fotos)} fotos")
        if a.simular:
            continue
        cid = existentes.get((padre, nombre))
        if cid is None:
            campos = {"name": nombre, "kind": "collection", "photo_ids": fotos[:LOTE]}
            if padre is not None:
                campos["parent_id"] = padre
            cid = control.pedir("create_collection", **campos)["created"]["id"]
            fotos = fotos[LOTE:]
        for i in range(0, len(fotos), LOTE):
            control.pedir("add_to_collection", collection_id=cid, photo_ids=fotos[i:i + LOTE])

    if sin_foto:
        print(f"Aviso: {sin_foto} fotos de álbumes no están en el catálogo (vídeos sin exportar o carpeta sin añadir).")


if __name__ == "__main__":
    main()
