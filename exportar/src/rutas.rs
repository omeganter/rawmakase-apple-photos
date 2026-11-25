//! Nombres y rutas de los archivos copiados. Funciones puras, con pruebas.

use std::collections::HashSet;

/// Nombre de archivo seguro también en exFAT y Windows.
pub fn nombre_seguro(s: &str) -> String {
    let limpio: String = s
        .chars()
        .map(|c| {
            if "/\\:*?\"<>|".contains(c) || c == '\n' || c == '\r' {
                '_'
            } else {
                c
            }
        })
        .collect();
    let limpio = limpio.trim_end_matches(['.', ' ']);
    if limpio.is_empty() {
        "sin-nombre".into()
    } else {
        limpio.into()
    }
}

/// Extensión de un nombre, sin el punto: "HEIC" en "IMG_0001.HEIC"; vacía si no tiene.
pub fn extension(nombre: &str) -> &str {
    match nombre.rfind('.') {
        Some(i) if i > 0 && i + 1 < nombre.len() => &nombre[i + 1..],
        _ => "",
    }
}

/// El nombre sin su extensión: "IMG_0001" en "IMG_0001.HEIC".
pub fn sin_extension(nombre: &str) -> &str {
    let ext = extension(nombre);
    if ext.is_empty() {
        nombre
    } else {
        &nombre[..nombre.len() - ext.len() - 1]
    }
}

/// Rutas relativas ya asignadas en esta exportación.
#[derive(Default)]
pub struct Rutas(HashSet<String>);

impl Rutas {
    /// Asigna una ruta relativa única para un archivo. Si ya está cogida (sin distinguir
    /// mayúsculas, como el disco), añade el principio del identificador de la foto, así que la
    /// misma fototeca da siempre las mismas rutas.
    pub fn asignar(&mut self, carpeta: &str, base: &str, ext: &str, id: &str) -> String {
        let con_ext = |base: &str| {
            if ext.is_empty() {
                format!("{carpeta}/{base}")
            } else {
                format!("{carpeta}/{base}.{ext}")
            }
        };
        let mut rel = con_ext(base);
        if self.0.contains(&rel.to_lowercase()) {
            let corto: String = id.chars().take(8).collect();
            rel = con_ext(&format!("{base}_{corto}"));
        }
        self.0.insert(rel.to_lowercase());
        rel
    }
}

#[cfg(test)]
mod pruebas {
    use super::*;

    #[test]
    fn los_nombres_quedan_validos_en_cualquier_disco() {
        assert_eq!(nombre_seguro("a/b:c?.jpg"), "a_b_c_.jpg");
        assert_eq!(nombre_seguro("foto. ."), "foto");
        assert_eq!(nombre_seguro(" ."), "sin-nombre");
    }

    #[test]
    fn la_extension_es_lo_que_sigue_al_ultimo_punto() {
        assert_eq!(
            (sin_extension("IMG_1.HEIC"), extension("IMG_1.HEIC")),
            ("IMG_1", "HEIC")
        );
        assert_eq!(
            (sin_extension("a.b.jpg"), extension("a.b.jpg")),
            ("a.b", "jpg")
        );
        assert_eq!((sin_extension("sin"), extension("sin")), ("sin", ""));
        assert_eq!(
            (sin_extension(".oculto"), extension(".oculto")),
            (".oculto", "")
        );
    }

    #[test]
    fn dos_fotos_con_el_mismo_nombre_no_se_pisan() {
        let mut rutas = Rutas::default();
        assert_eq!(
            rutas.asignar("2019/2019-10", "IMG_1", "JPG", "AAAAAAAA-1111"),
            "2019/2019-10/IMG_1.JPG"
        );
        assert_eq!(
            rutas.asignar("2019/2019-10", "img_1", "jpg", "BBBBBBBB-2222"),
            "2019/2019-10/img_1_BBBBBBBB.jpg"
        );
        assert_eq!(
            rutas.asignar("2019/2019-10", "IMG_1", "", "CCCCCCCC-3333"),
            "2019/2019-10/IMG_1"
        );
    }
}
