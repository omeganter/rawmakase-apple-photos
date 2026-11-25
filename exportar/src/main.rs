//! fotos-a-rawmakase · lector de la fototeca
//!
//! Copia la fototeca de Fotos (macOS) a una carpeta normal, sin modificar la fototeca:
//!
//! ```text
//! AAAA/AAAA-MM/IMG_0001.HEIC          original, byte a byte
//! AAAA/AAAA-MM/IMG_0001_editada.jpg   versión editada tal como la muestra Fotos (si la hay)
//! AAAA/AAAA-MM/IMG_0001.HEIC.xmp      título, descripción, palabras clave, personas, "Favorito",
//!                                     y la fecha y el lugar tal como los muestra Fotos
//! AAAA/AAAA-MM/IMG_0001.MOV           vídeo de la Live Photo (con --videos)
//! _fotos/ajustes/<id>.plist           datos del ajuste de Apple, por si se convierten más adelante
//! _fotos/manifiesto.json              fotos, álbumes y carpetas, con su orden
//! ```
//!
//! Uso (dentro del .app, por el permiso de Fotos):
//!
//! ```text
//! open -n FotosARawmakase.app --args --dest /ruta/destino [--album "Nombre"] [--limit N]
//!     [--videos] [--rafagas] [--ocultas] [--dry-run] [--solo-editadas] [--solo-con-metadatos]
//! ```
//!
//! Códigos de salida: 0 bien, 1 con errores de copia, 2 falta `--dest`, 3 sin permiso de Fotos,
//! 5 cancelado.

mod bd;
mod fotos;
mod rutas;
mod xmp;

use std::collections::HashSet;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::exit;

use objc2::rc::autoreleasepool;
use serde_json::{Map, Value, json};

use fotos::{Clase, Coleccion, Recurso};
use rutas::{Rutas, extension, nombre_seguro, sin_extension};
use xmp::Meta;

#[derive(Default)]
struct Opciones {
    destino: String,
    album: Option<String>,
    limite: Option<usize>,
    videos: bool,
    simulacion: bool,
    /// Todas las fotos de cada ráfaga, no solo la elegida.
    rafagas: bool,
    /// También las del álbum Oculto.
    ocultas: bool,
    /// Para pruebas: solo fotos con ediciones.
    solo_editadas: bool,
    /// Para pruebas: solo fotos con título, descripción, palabras clave o personas.
    solo_con_metadatos: bool,
}

fn opciones() -> Opciones {
    let mut o = Opciones::default();
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--dest" => o.destino = args.next().unwrap_or_default(),
            "--album" => o.album = args.next(),
            "--limit" => o.limite = args.next().and_then(|n| n.parse().ok()),
            "--videos" => o.videos = true,
            "--dry-run" => o.simulacion = true,
            "--solo-editadas" => o.solo_editadas = true,
            "--solo-con-metadatos" => o.solo_con_metadatos = true,
            "--rafagas" => o.rafagas = true,
            "--ocultas" => o.ocultas = true,
            _ => {}
        }
    }
    o
}

/// El registro: cada línea va al archivo de `_fotos` y a la salida estándar.
struct Registro(Option<fs::File>);

impl Registro {
    fn linea(&mut self, s: &str) {
        if let Some(f) = &mut self.0 {
            let _ = writeln!(f, "{s}");
        }
        println!("{s}");
    }
}

fn existe(ruta: &Path) -> bool {
    fs::metadata(ruta).is_ok_and(|m| m.len() > 0)
}

/// Escribe un recurso de Fotos en `ruta`, primero a un archivo parcial.
fn escribir(recurso: &Recurso, ruta: &Path) -> Result<(), String> {
    let mut parcial = ruta.as_os_str().to_owned();
    parcial.push(".parcial");
    let parcial = PathBuf::from(parcial);
    let _ = fs::remove_file(&parcial);
    if let Err(e) = recurso.escribir(&parcial) {
        let _ = fs::remove_file(&parcial);
        return Err(e);
    }
    fs::rename(&parcial, ruta).map_err(|e| e.to_string())
}

/// Cuentas de la exportación.
#[derive(Default)]
struct Cuentas {
    copiados: u64,
    ya_estaban: u64,
    errores: u64,
    editadas: u64,
    bytes: u64,
}

struct Exportacion {
    op: Opciones,
    destino: PathBuf,
    registro: Registro,
    rutas: Rutas,
    cuentas: Cuentas,
}

impl Exportacion {
    /// Copia un recurso si aún no está. Devuelve `false` si falla.
    fn copiar(&mut self, recurso: &Recurso, rel: &str) -> bool {
        let ruta = self.destino.join(rel);
        if existe(&ruta) {
            self.cuentas.ya_estaban += 1;
            return true;
        }
        if self.op.simulacion {
            self.cuentas.copiados += 1;
            return true;
        }
        if let Some(carpeta) = ruta.parent() {
            let _ = fs::create_dir_all(carpeta);
        }
        if let Err(e) = escribir(recurso, &ruta) {
            self.cuentas.errores += 1;
            self.registro.linea(&format!("ERROR {rel}: {e}"));
            return false;
        }
        self.cuentas.copiados += 1;
        self.cuentas.bytes += fs::metadata(&ruta).map_or(0, |m| m.len());
        true
    }

    fn progreso(&self, hechos: usize, total: usize) -> String {
        let c = &self.cuentas;
        format!(
            "{hechos}/{total} · copiados {} · ya estaban {} · errores {}",
            c.copiados, c.ya_estaban, c.errores
        )
    }
}

/// Pasa el árbol de Fotos a la lista plana del manifiesto. Con `--album`, solo ese álbum y las
/// carpetas que lo contienen. Devuelve `true` si algo de `nodos` quedó en la lista.
fn aplanar(
    nodos: &[Coleccion],
    padre: Option<&str>,
    filtro: Option<&str>,
    lista: &mut Vec<Value>,
    queridas: &mut HashSet<String>,
) -> bool {
    let mut alguno = false;
    for nodo in nodos {
        match nodo {
            Coleccion::Carpeta { id, nombre, hijos } => {
                let posicion = lista.len();
                lista.push(json!({"id": id, "nombre": nombre, "tipo": "carpeta", "padre": padre}));
                if aplanar(hijos, Some(id), filtro, lista, queridas) || filtro.is_none() {
                    alguno = true;
                } else {
                    // Con filtro, las carpetas vacías sobran.
                    lista.truncate(posicion);
                }
            }
            Coleccion::Album { id, nombre, fotos } => {
                if filtro.is_some_and(|f| f != nombre) {
                    continue;
                }
                queridas.extend(fotos.iter().cloned());
                lista.push(json!({"id": id, "nombre": nombre, "tipo": "album", "padre": padre, "fotos": fotos}));
                alguno = true;
            }
        }
    }
    alguno
}

fn main() {
    let op = opciones();
    if op.destino.is_empty() {
        eprintln!("Falta --dest /ruta/destino");
        exit(2);
    }
    let destino = PathBuf::from(&op.destino);
    let meta_dir = destino.join("_fotos");
    let _ = fs::create_dir_all(meta_dir.join("ajustes"));
    let registro = Registro(
        fs::File::create(meta_dir.join(if op.simulacion {
            "simulacion.log"
        } else {
            "exportacion.log"
        }))
        .ok(),
    );
    let mut ex = Exportacion {
        op,
        destino,
        registro,
        rutas: Rutas::default(),
        cuentas: Cuentas::default(),
    };

    if let Err(estado) = fotos::pedir_permiso() {
        ex.registro.linea(&format!("Sin permiso de acceso completo a Fotos (estado {estado}). Actívalo en Ajustes del Sistema > Privacidad y seguridad > Fotos."));
        exit(3);
    }
    exit(exportar(&mut ex, &meta_dir));
}

fn exportar(ex: &mut Exportacion, meta_dir: &Path) -> i32 {
    let bd = bd::leer(fotos::version_de_macos());
    ex.registro.linea(&bd.resumen);
    let meta_de = |id: &str| bd.metas.get(id.get(..36).unwrap_or(id));

    // 1. Carpetas y álbumes del usuario, con su jerarquía y el orden de sus fotos.
    let mut colecciones = Vec::new();
    let mut queridas = HashSet::new(); // con --album: solo las fotos de esos álbumes
    aplanar(
        &fotos::colecciones(),
        None,
        ex.op.album.as_deref(),
        &mut colecciones,
        &mut queridas,
    );

    // 2. Fotos a exportar, de la más antigua a la más reciente.
    let mut elementos = fotos::elementos_de_la_fototeca(ex.op.rafagas, ex.op.ocultas);
    elementos.retain(|e| {
        let id = e.id();
        (ex.op.album.is_none() || queridas.contains(&id))
            && !e.es_otro()
            && (ex.op.videos || !e.es_video())
            && (!ex.op.solo_con_metadatos || meta_de(&id).is_some_and(|m| !m.vacio()))
            && (!ex.op.solo_editadas || e.recursos().iter().any(|r| r.clase() == Clase::Ajuste))
    });
    if let Some(n) = ex.op.limite {
        elementos.truncate(n);
    }
    let total = elementos.len();
    ex.registro.linea(&format!(
        "Elementos a exportar: {total}{}",
        if ex.op.simulacion {
            " (simulación, no se copia nada)"
        } else {
            ""
        }
    ));

    let cancelar = meta_dir.join("cancelar");
    let mut manifiesto: Vec<Value> = Vec::new();
    for (indice, elemento) in elementos.iter().enumerate() {
        // Quien lanzó la exportación puede pedir que pare creando este archivo. Lo copiado se queda.
        if cancelar.exists() {
            let _ = fs::remove_file(&cancelar);
            let linea = format!("CANCELADO · {}", ex.progreso(indice, total));
            ex.registro.linea(&linea);
            return 5;
        }
        // Sin esto, los objetos de Fotos de cada vuelta se acumularían hasta el final.
        autoreleasepool(|_| {
            if let Some(entrada) = exportar_elemento(ex, elemento, meta_de(&elemento.id())) {
                manifiesto.push(Value::Object(entrada));
            }
        });
        if (indice + 1) % 100 == 0 || indice + 1 == total {
            let linea = ex.progreso(indice + 1, total);
            ex.registro.linea(&linea);
        }
    }

    // 3. Manifiesto
    let ahora = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0.0, |d| d.as_secs_f64());
    let albumes = colecciones.iter().filter(|c| c["tipo"] == "album").count();
    let documento = json!({
        "version": 2,
        "herramienta": concat!("fotos-a-rawmakase ", env!("CARGO_PKG_VERSION")),
        "generado": xmp::iso_utc(ahora),
        "origen": bd.origen,
        "opciones": {"videos": ex.op.videos, "rafagas": ex.op.rafagas, "ocultas": ex.op.ocultas, "album": ex.op.album},
        "destino": ex.destino.to_string_lossy(),
        "fotos": manifiesto,
        "colecciones": colecciones,
    });
    // Primero a un archivo temporal: quien lo lea nunca lo ve a medias.
    let nombre = if ex.op.simulacion {
        "manifiesto-simulacion.json"
    } else {
        "manifiesto.json"
    };
    let temporal = meta_dir.join(format!("{nombre}.parcial"));
    if serde_json::to_vec_pretty(&documento)
        .ok()
        .and_then(|datos| fs::write(&temporal, datos).ok())
        .is_some()
    {
        let _ = fs::rename(&temporal, meta_dir.join(nombre));
    }
    let c = &ex.cuentas;
    let linea = format!(
        "FIN · elementos {total} · archivos copiados {} · ya estaban {} · editadas {} · errores {} · {} MB · álbumes {albumes}",
        c.copiados,
        c.ya_estaban,
        c.editadas,
        c.errores,
        c.bytes / 1_000_000
    );
    let errores = c.errores;
    ex.registro.linea(&linea);
    i32::from(errores != 0)
}

/// Copia los archivos de un elemento y devuelve su entrada del manifiesto. `None` si Fotos no
/// tiene su original.
fn exportar_elemento(
    ex: &mut Exportacion,
    elemento: &fotos::Elemento,
    meta: Option<&Meta>,
) -> Option<Map<String, Value>> {
    let id = elemento.id();
    let recursos = elemento.recursos();
    let de_clase = |clase: Clase| recursos.iter().find(|r| r.clase() == clase);
    let es_video = elemento.es_video();
    let Some(original) = de_clase(if es_video { Clase::Video } else { Clase::Foto }) else {
        ex.cuentas.errores += 1;
        ex.registro
            .linea(&format!("ERROR {id}: sin recurso original"));
        return None;
    };
    let fecha = elemento.fecha();
    let carpeta = fecha.map_or_else(
        || "sin-fecha".to_string(),
        |f| xmp::carpeta(f, elemento.desfase_local()),
    );
    let nombre = nombre_seguro(&original.nombre());

    let mut entrada = Map::new();
    entrada.insert("id".into(), json!(id));
    entrada.insert(
        "tipo".into(),
        json!(if es_video { "video" } else { "foto" }),
    );
    entrada.insert("favorito".into(), json!(elemento.favorito()));
    if let Some(f) = fecha {
        entrada.insert("fecha".into(), json!(xmp::iso_utc(f)));
    }

    let rel_original = ex
        .rutas
        .asignar(&carpeta, sin_extension(&nombre), extension(&nombre), &id);
    if ex.copiar(original, &rel_original) {
        entrada.insert("original".into(), json!(rel_original));
    }
    // Nombre sin carpeta ni extensión, con el sufijo de colisión si lo hubo: los demás archivos
    // del elemento lo comparten.
    let base = sin_extension(rel_original.rsplit('/').next().unwrap_or(&rel_original)).to_string();

    // Cada archivo acompañante: (clase, clave del manifiesto, sufijo del nombre, extensión si
    // Fotos no da ninguna).
    let mut acompanantes = vec![
        (Clase::Raw, "raw", "", ""),
        (
            if es_video {
                Clase::VideoEditado
            } else {
                Clase::FotoEditada
            },
            "editada",
            "_editada",
            "jpg",
        ),
    ];
    if ex.op.videos && !es_video {
        acompanantes.push((Clase::VideoLive, "video_live", "", "mov"));
        acompanantes.push((
            Clase::VideoLiveEditado,
            "video_live_editado",
            "_editada",
            "mov",
        ));
    }
    for (clase, clave, sufijo, ext_por_defecto) in acompanantes {
        let Some(recurso) = de_clase(clase) else {
            continue;
        };
        let nombre = recurso.nombre();
        let ext = if extension(&nombre).is_empty() {
            ext_por_defecto
        } else {
            extension(&nombre)
        };
        let rel = ex
            .rutas
            .asignar(&carpeta, &format!("{base}{sufijo}"), ext, &id);
        if ex.copiar(recurso, &rel) {
            entrada.insert(clave.into(), json!(rel));
            if clave == "editada" {
                ex.cuentas.editadas += 1;
            }
        }
    }

    if elemento.live() {
        entrada.insert("live".into(), json!(true));
    }
    if let Some(rafaga) = elemento.rafaga() {
        entrada.insert("rafaga".into(), json!(rafaga));
    }
    if elemento.oculta() {
        entrada.insert("oculta".into(), json!(true));
    }
    let lugar = elemento.lugar();
    if let Some((latitud, longitud)) = lugar {
        entrada.insert("lugar".into(), json!([latitud, longitud]));
    }
    let vacio = Meta::default();
    let meta = meta.unwrap_or(&vacio);
    if let Some((f, d)) = fecha.zip(meta.desfase) {
        entrada.insert("fecha_local".into(), json!(xmp::fecha_local(f, d)));
    }

    // Datos del ajuste de Apple (no los lee RAWmakase; se guardan para el futuro).
    if let Some(ajuste) = de_clase(Clase::Ajuste) {
        let rel = format!("_fotos/ajustes/{}.plist", nombre_seguro(&id));
        let ruta = ex.destino.join(&rel);
        if existe(&ruta) || (!ex.op.simulacion && escribir(ajuste, &ruta).is_ok()) {
            entrada.insert("ajuste".into(), json!(rel));
        }
    }

    // Lo que no viaja dentro del archivo, en un sidecar XMP que RAWmakase lee al añadir la foto.
    // "IMG_0001.JPG.xmp" vale para cualquier formato; "IMG_0001.xmp" solo lo lee para los RAW.
    if let Some(t) = &meta.titulo {
        entrada.insert("titulo".into(), json!(t));
    }
    if let Some(d) = &meta.descripcion {
        entrada.insert("descripcion".into(), json!(d));
    }
    if !meta.palabras_clave.is_empty() {
        entrada.insert("palabras_clave".into(), json!(meta.palabras_clave));
    }
    if !meta.personas.is_empty() {
        entrada.insert("personas".into(), json!(meta.personas));
    }
    if !ex.op.simulacion
        && let Some(texto) = xmp::sidecar(meta, elemento.favorito(), fecha, lugar)
    {
        for clave in ["original", "raw", "editada"] {
            let Some(rel) = entrada.get(clave).and_then(Value::as_str) else {
                continue;
            };
            let ruta = ex.destino.join(format!("{rel}.xmp"));
            // Nunca se pisa un sidecar que no sea nuestro.
            if fs::read_to_string(&ruta)
                .map_or(!ruta.exists(), |previo| previo.contains(xmp::MARCA))
            {
                let _ = fs::write(&ruta, &texto);
            }
        }
    }
    Some(entrada)
}
