//! Base de datos de Fotos (`Photos.sqlite`), en solo lectura.
//!
//! PhotoKit no da títulos, descripciones, palabras clave, personas ni la zona horaria, así que se
//! leen de la base de datos de la fototeca. No es una interfaz oficial: Apple cambia tablas y
//! columnas entre versiones de macOS. Por eso:
//!   · las columnas que cambian de nombre se buscan en la propia base de datos, no se dan por
//!     sabidas;
//!   · cada versión de macOS comprobada a mano se apunta en `MACOS_COMPROBADOS`;
//!   · el manifiesto registra la versión de macOS, la del esquema y qué columnas se usaron, para
//!     que quien lo importe sepa de dónde salen los datos y si esa versión estaba comprobada.
//!
//! Para dar por buena otra versión de macOS: exportar, revisar el bloque "origen" del manifiesto
//! y unas cuantas fotos con metadatos, y añadir su número aquí.

use std::collections::HashMap;

use rusqlite::types::ValueRef;
use rusqlite::{Connection, OpenFlags};
use serde_json::{Map, Value, json};

use crate::xmp::Meta;

/// Versiones mayores de macOS en las que se ha comprobado la lectura de `Photos.sqlite`.
pub const MACOS_COMPROBADOS: &[i64] = &[27];

/// Lo leído de la base de datos.
#[derive(Default)]
pub struct Lectura {
    /// Metadatos por UUID de la foto (los 36 primeros caracteres de su identificador).
    pub metas: HashMap<String, Meta>,
    /// Bloque "origen" del manifiesto.
    pub origen: Map<String, Value>,
    /// Línea de resumen para el registro.
    pub resumen: String,
}

/// Filas de una consulta, con cada valor como texto. `None` si la consulta no se puede preparar.
fn filas(bd: &Connection, sql: &str) -> Option<Vec<Vec<Option<String>>>> {
    let mut consulta = bd.prepare(sql).ok()?;
    let columnas = consulta.column_count();
    let mut filas = consulta.query([]).ok()?;
    let mut salida = Vec::new();
    while let Ok(Some(fila)) = filas.next() {
        salida.push(
            (0..columnas)
                .map(|i| match fila.get_ref(i).ok()? {
                    ValueRef::Null | ValueRef::Blob(_) => None,
                    ValueRef::Integer(n) => Some(n.to_string()),
                    ValueRef::Real(x) => Some(x.to_string()),
                    ValueRef::Text(t) => Some(String::from_utf8_lossy(t).into_owned()),
                })
                .collect(),
        );
    }
    Some(salida)
}

/// Primera columna, no nula, de cada fila.
fn primera_columna(bd: &Connection, sql: &str) -> Vec<String> {
    filas(bd, sql)
        .unwrap_or_default()
        .into_iter()
        .filter_map(|mut f| f.swap_remove(0))
        .collect()
}

fn columnas(bd: &Connection, tabla: &str) -> Vec<String> {
    primera_columna(
        bd,
        &format!("SELECT name FROM pragma_table_info('{tabla}')"),
    )
}

/// Pares (UUID de la foto, valor) de una consulta de dos columnas.
fn pares(bd: &Connection, sql: &str) -> Vec<(String, String)> {
    filas(bd, sql)
        .unwrap_or_default()
        .into_iter()
        .filter_map(|mut f| {
            let valor = f.pop()??;
            let uuid = f.pop()??;
            Some((uuid, valor))
        })
        .collect()
}

/// Ruta como URI de SQLite: todo lo que no sea letra, cifra, `/`, `-`, `.`, `_` o `~` va escapado.
fn uri(ruta: &str) -> String {
    let mut s = String::from("file:");
    for b in ruta.bytes() {
        if b.is_ascii_alphanumeric() || b"/-._~".contains(&b) {
            s.push(b as char);
        } else {
            s += &format!("%{b:02X}");
        }
    }
    s + "?mode=ro"
}

/// Lee los metadatos de la fototeca del sistema. `macos` es (mayor, menor, parche).
/// Si la base de datos no se puede abrir, devuelve la lectura vacía con un aviso en `resumen`.
pub fn leer(macos: (i64, i64, i64)) -> Lectura {
    let mut out = Lectura::default();
    let comprobado = MACOS_COMPROBADOS.contains(&macos.0);
    let version = format!("{}.{}.{}", macos.0, macos.1, macos.2);
    out.origen.insert("macos".into(), json!(version));
    out.origen
        .insert("macos_comprobado".into(), json!(comprobado));
    out.origen.insert("metadatos".into(), json!("no leídos"));

    let casa = std::env::var("HOME").unwrap_or_default();
    let ruta = format!("{casa}/Pictures/Photos Library.photoslibrary/database/Photos.sqlite");
    let abierta = Connection::open_with_flags(
        uri(&ruta),
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI,
    );
    // Abrir no lee nada todavía: la primera consulta dice si de verdad se puede leer.
    let bd = match abierta {
        Ok(bd) if filas(&bd, "SELECT 1 FROM sqlite_master LIMIT 1").is_some() => bd,
        _ => {
            out.resumen = "Aviso: no se puede abrir la base de datos de Fotos; se exporta sin títulos, descripciones, palabras clave ni personas.".into();
            return out;
        }
    };
    let _ = bd.busy_timeout(std::time::Duration::from_secs(5));

    let mut esquema = Map::new();
    if let Some(z) = primera_columna(&bd, "SELECT Z_VERSION FROM Z_METADATA")
        .into_iter()
        .next()
    {
        esquema.insert("z_version".into(), json!(z));
    }
    let mut usado: Map<String, Value> = Map::new();
    let mut faltan: Vec<&str> = Vec::new();
    let tiene = |columnas: &[String], nombre: &str| columnas.iter().any(|c| c == nombre);
    let primera = |columnas: &[String], candidatas: &[&str]| -> Option<String> {
        candidatas
            .iter()
            .find(|c| tiene(columnas, c))
            .map(|c| (*c).to_string())
    };

    // Títulos
    let attr = columnas(&bd, "ZADDITIONALASSETATTRIBUTES");
    if tiene(&attr, "ZASSET") && tiene(&attr, "ZTITLE") && tiene(&columnas(&bd, "ZASSET"), "ZUUID")
    {
        for (u, t) in pares(
            &bd,
            "SELECT a.ZUUID, x.ZTITLE FROM ZADDITIONALASSETATTRIBUTES x JOIN ZASSET a ON a.Z_PK = x.ZASSET WHERE x.ZTITLE IS NOT NULL AND x.ZTITLE <> ''",
        ) {
            out.metas.entry(u).or_default().titulo = Some(t);
        }
        usado.insert("titulo".into(), json!("ZADDITIONALASSETATTRIBUTES.ZTITLE"));
    } else {
        faltan.push("titulo");
    }

    // Desfase horario de cada foto, para escribir la fecha tal como la muestra Fotos.
    if tiene(&attr, "ZASSET") && tiene(&attr, "ZTIMEZONEOFFSET") {
        for (u, t) in pares(
            &bd,
            "SELECT a.ZUUID, x.ZTIMEZONEOFFSET FROM ZADDITIONALASSETATTRIBUTES x JOIN ZASSET a ON a.Z_PK = x.ZASSET WHERE x.ZTIMEZONEOFFSET IS NOT NULL",
        ) {
            if let Ok(segundos) = t.parse::<i64>() {
                out.metas.entry(u).or_default().desfase = Some(segundos);
            }
        }
        usado.insert(
            "zona_horaria".into(),
            json!("ZADDITIONALASSETATTRIBUTES.ZTIMEZONEOFFSET"),
        );
    } else {
        faltan.push("zona_horaria");
    }

    // Descripciones
    let desc = columnas(&bd, "ZASSETDESCRIPTION");
    if tiene(&desc, "ZLONGDESCRIPTION")
        && tiene(&desc, "ZASSETATTRIBUTES")
        && tiene(&attr, "ZASSET")
    {
        for (u, d) in pares(
            &bd,
            "SELECT a.ZUUID, d.ZLONGDESCRIPTION FROM ZASSETDESCRIPTION d JOIN ZADDITIONALASSETATTRIBUTES x ON x.Z_PK = d.ZASSETATTRIBUTES JOIN ZASSET a ON a.Z_PK = x.ZASSET WHERE d.ZLONGDESCRIPTION IS NOT NULL AND d.ZLONGDESCRIPTION <> ''",
        ) {
            out.metas.entry(u).or_default().descripcion = Some(d);
        }
        usado.insert(
            "descripcion".into(),
            json!("ZASSETDESCRIPTION.ZLONGDESCRIPTION"),
        );
    } else {
        faltan.push("descripcion");
    }

    // Palabras clave: la tabla de unión se llama Z_<n>KEYWORDS y sus columnas llevan números
    // que cambian.
    let union = primera_columna(&bd, "SELECT name FROM sqlite_master WHERE type='table' AND name LIKE 'Z\\_%KEYWORDS' ESCAPE '\\' ORDER BY name").into_iter().next();
    let claves = union.as_ref().and_then(|union| {
        let cols = columnas(&bd, union);
        let c_attr = cols
            .iter()
            .find(|c| c.ends_with("ASSETATTRIBUTES"))?
            .clone();
        let c_clave = cols.iter().find(|c| c.ends_with("KEYWORDS"))?.clone();
        tiene(&columnas(&bd, "ZKEYWORD"), "ZTITLE").then_some((union.clone(), c_attr, c_clave))
    });
    if let Some((union, c_attr, c_clave)) = claves {
        for (u, k) in pares(
            &bd,
            &format!(
                "SELECT a.ZUUID, k.ZTITLE FROM {union} j JOIN ZKEYWORD k ON k.Z_PK = j.{c_clave} JOIN ZADDITIONALASSETATTRIBUTES x ON x.Z_PK = j.{c_attr} JOIN ZASSET a ON a.Z_PK = x.ZASSET WHERE k.ZTITLE IS NOT NULL AND k.ZTITLE <> '' ORDER BY k.ZTITLE"
            ),
        ) {
            out.metas.entry(u).or_default().palabras_clave.push(k);
        }
        usado.insert(
            "palabras_clave".into(),
            json!(format!("{union}.{c_attr}/{c_clave}")),
        );
    } else {
        faltan.push("palabras_clave");
    }

    // Personas con nombre: la columna de la cara hacia la foto y hacia la persona ha cambiado
    // de nombre entre versiones.
    let cara = columnas(&bd, "ZDETECTEDFACE");
    let persona = columnas(&bd, "ZPERSON");
    let nombres = (|| {
        Some((
            primera(&cara, &["ZASSETFORFACE", "ZASSET"])?,
            primera(&cara, &["ZPERSONFORFACE", "ZPERSON"])?,
            primera(&persona, &["ZFULLNAME", "ZDISPLAYNAME"])?,
        ))
    })();
    if let Some((c_foto, c_persona, c_nombre)) = nombres {
        for (u, n) in pares(
            &bd,
            &format!(
                "SELECT DISTINCT a.ZUUID, p.{c_nombre} FROM ZDETECTEDFACE f JOIN ZPERSON p ON p.Z_PK = f.{c_persona} JOIN ZASSET a ON a.Z_PK = f.{c_foto} WHERE p.{c_nombre} IS NOT NULL AND p.{c_nombre} <> '' ORDER BY p.{c_nombre}"
            ),
        ) {
            out.metas.entry(u).or_default().personas.push(n);
        }
        usado.insert(
            "personas".into(),
            json!(format!(
                "ZDETECTEDFACE.{c_foto}/{c_persona}, ZPERSON.{c_nombre}"
            )),
        );
    } else {
        faltan.push("personas");
    }

    esquema.insert("columnas".into(), Value::Object(usado));
    if !faltan.is_empty() {
        esquema.insert("no_encontrado".into(), json!(faltan));
    }
    out.origen
        .insert("esquema_fotos".into(), Value::Object(esquema));
    out.origen.insert(
        "metadatos".into(),
        json!(if faltan.is_empty() {
            "base de datos de Fotos"
        } else {
            "base de datos de Fotos (incompleto)"
        }),
    );

    let cuenta = |f: fn(&Meta) -> bool| out.metas.values().filter(|m| f(m)).count();
    out.resumen = format!(
        "Base de datos de Fotos · macOS {version}{} · con título {} · con descripción {} · con palabras clave {} · con personas {} · con zona horaria {}{}",
        if comprobado {
            ""
        } else {
            " (versión NO comprobada)"
        },
        cuenta(|m| m.titulo.is_some()),
        cuenta(|m| m.descripcion.is_some()),
        cuenta(|m| !m.palabras_clave.is_empty()),
        cuenta(|m| !m.personas.is_empty()),
        cuenta(|m| m.desfase.is_some()),
        if faltan.is_empty() {
            String::new()
        } else {
            format!(" · no encontrado: {}", faltan.join(", "))
        },
    );
    out
}

#[cfg(test)]
mod pruebas {
    use super::*;

    #[test]
    fn la_ruta_de_la_fototeca_se_escapa_para_sqlite() {
        assert_eq!(
            uri("/Users/a b/Pictures/Photos Library.photoslibrary/database/Photos.sqlite"),
            "file:/Users/a%20b/Pictures/Photos%20Library.photoslibrary/database/Photos.sqlite?mode=ro"
        );
        assert_eq!(uri("/tmp/ñ?#"), "file:/tmp/%C3%B1%3F%23?mode=ro");
    }

    #[test]
    fn los_valores_se_leen_como_texto_y_los_nulos_se_saltan() {
        let bd = Connection::open_in_memory().unwrap();
        bd.execute_batch("CREATE TABLE t(u TEXT, v); INSERT INTO t VALUES('a', 7200), ('b', NULL), (NULL, 'x'), ('c', 'hola');").unwrap();
        assert_eq!(
            pares(&bd, "SELECT u, v FROM t ORDER BY rowid"),
            vec![("a".into(), "7200".into()), ("c".into(), "hola".into())]
        );
        assert_eq!(columnas(&bd, "t"), vec!["u".to_string(), "v".to_string()]);
        assert!(filas(&bd, "SELECT * FROM no_existe").is_none());
    }
}
