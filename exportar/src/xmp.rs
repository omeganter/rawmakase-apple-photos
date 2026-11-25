//! Sidecar XMP y fechas. Nada aquí toca Fotos: son funciones puras, con pruebas.

/// Metadatos de una foto que no viajan dentro del archivo.
#[derive(Default, Clone, Debug)]
pub struct Meta {
    pub titulo: Option<String>,
    pub descripcion: Option<String>,
    pub palabras_clave: Vec<String>,
    pub personas: Vec<String>,
    /// Desfase horario (segundos) con el que Fotos muestra la fecha de la foto.
    pub desfase: Option<i64>,
}

impl Meta {
    /// Sin título, descripción, palabras clave ni personas (el desfase no cuenta).
    pub fn vacio(&self) -> bool {
        self.titulo.is_none()
            && self.descripcion.is_none()
            && self.palabras_clave.is_empty()
            && self.personas.is_empty()
    }
}

/// Marca que distingue nuestros sidecars de los de otros programas.
pub const MARCA: &str = "<!-- fotos-a-rawmakase -->";

/// Fecha civil (calendario gregoriano) de un instante, visto con un desfase fijo.
pub struct Civil {
    pub anno: i64,
    pub mes: i64,
    pub dia: i64,
    pub hora: i64,
    pub minuto: i64,
    pub segundo: i64,
}

/// `unix` son segundos desde 1970 en UTC; `desfase`, los segundos al este de UTC.
pub fn civil(unix: f64, desfase: i64) -> Civil {
    // Como los formateadores de fechas de Apple: se redondea al milisegundo antes de quitar la
    // fracción, así que 4,9996 s se escribe como 5 s.
    let t = ((unix * 1_000.0).round() / 1_000.0).floor() as i64 + desfase;
    let (dias, resto) = (t.div_euclid(86_400), t.rem_euclid(86_400));
    // Algoritmo de días a fecha civil de Howard Hinnant.
    let z = dias + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let dia = doy - (153 * mp + 2) / 5 + 1;
    let mes = if mp < 10 { mp + 3 } else { mp - 9 };
    let anno = yoe + era * 400 + i64::from(mes <= 2);
    Civil {
        anno,
        mes,
        dia,
        hora: resto / 3_600,
        minuto: resto % 3_600 / 60,
        segundo: resto % 60,
    }
}

/// Carpeta de una foto: "2019/2019-10".
pub fn carpeta(unix: f64, desfase: i64) -> String {
    let c = civil(unix, desfase);
    format!("{:04}/{:04}-{:02}", c.anno, c.anno, c.mes)
}

/// Instante en UTC: "2019-10-05T16:22:11Z".
pub fn iso_utc(unix: f64) -> String {
    let c = civil(unix, 0);
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        c.anno, c.mes, c.dia, c.hora, c.minuto, c.segundo
    )
}

/// Fecha de captura como la muestra Fotos (también si se corrigió allí), con su desfase:
/// "2019-10-05T18:22:11+02:00".
pub fn fecha_local(unix: f64, desfase: i64) -> String {
    let c = civil(unix, desfase);
    let a = desfase.abs();
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}{}{:02}:{:02}",
        c.anno,
        c.mes,
        c.dia,
        c.hora,
        c.minuto,
        c.segundo,
        if desfase < 0 { '-' } else { '+' },
        a / 3_600,
        a % 3_600 / 60
    )
}

/// Coordenada en el formato de XMP: "41,23.456700N".
pub fn coordenada(valor: f64, positivo: char, negativo: char) -> String {
    let a = valor.abs();
    let grados = a.trunc();
    format!(
        "{},{:.6}{}",
        grados as i64,
        (a - grados) * 60.0,
        if valor < 0.0 { negativo } else { positivo }
    )
}

fn escapar(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn bolsa(elementos: &[String]) -> String {
    elementos
        .iter()
        .map(|e| format!("<rdf:li>{}</rdf:li>", escapar(e)))
        .collect()
}

/// Sidecar que RAWmakase lee al añadir la foto. Las personas van como palabras clave bajo
/// "Personas". `None` si no hay nada que escribir.
pub fn sidecar(
    meta: &Meta,
    favorito: bool,
    unix: Option<f64>,
    lugar: Option<(f64, f64)>,
) -> Option<String> {
    let mut planas: Vec<String> = Vec::new();
    if favorito {
        planas.push("Favorito".into());
    }
    planas.extend(meta.palabras_clave.iter().cloned());
    planas.extend(meta.personas.iter().cloned());
    let cuando = unix.zip(meta.desfase).map(|(u, d)| fecha_local(u, d));
    if planas.is_empty()
        && meta.titulo.is_none()
        && meta.descripcion.is_none()
        && cuando.is_none()
        && lugar.is_none()
    {
        return None;
    }
    let mut cuerpo = String::new();
    if let Some(t) = &meta.titulo {
        cuerpo += &format!(
            "   <dc:title><rdf:Alt><rdf:li xml:lang=\"x-default\">{}</rdf:li></rdf:Alt></dc:title>\n",
            escapar(t)
        );
    }
    if let Some(d) = &meta.descripcion {
        cuerpo += &format!(
            "   <dc:description><rdf:Alt><rdf:li xml:lang=\"x-default\">{}</rdf:li></rdf:Alt></dc:description>\n",
            escapar(d)
        );
    }
    if !planas.is_empty() {
        cuerpo += &format!(
            "   <dc:subject><rdf:Bag>{}</rdf:Bag></dc:subject>\n",
            bolsa(&planas)
        );
    }
    if !meta.personas.is_empty() {
        let bajo: Vec<String> = meta
            .personas
            .iter()
            .map(|p| format!("Personas|{p}"))
            .collect();
        cuerpo += &format!(
            "   <lr:hierarchicalSubject><rdf:Bag>{}</rdf:Bag></lr:hierarchicalSubject>\n",
            bolsa(&bajo)
        );
    }
    if let Some(c) = cuando {
        cuerpo += &format!("   <exif:DateTimeOriginal>{c}</exif:DateTimeOriginal>\n");
    }
    if let Some((lat, lon)) = lugar {
        cuerpo += &format!(
            "   <exif:GPSLatitude>{}</exif:GPSLatitude>\n",
            coordenada(lat, 'N', 'S')
        );
        cuerpo += &format!(
            "   <exif:GPSLongitude>{}</exif:GPSLongitude>\n",
            coordenada(lon, 'E', 'W')
        );
    }
    Some(format!(
        "<?xpacket begin=\"\" id=\"W5M0MpCehiHzreSzNTczkc9d\"?>\n{MARCA}\n<x:xmpmeta xmlns:x=\"adobe:ns:meta/\">\n <rdf:RDF xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\">\n  <rdf:Description rdf:about=\"\" xmlns:dc=\"http://purl.org/dc/elements/1.1/\" xmlns:lr=\"http://ns.adobe.com/lightroom/1.0/\" xmlns:exif=\"http://ns.adobe.com/exif/1.0/\">\n{cuerpo}  </rdf:Description>\n </rdf:RDF>\n</x:xmpmeta>\n<?xpacket end=\"w\"?>\n"
    ))
}

#[cfg(test)]
mod pruebas {
    use super::*;

    // 2019-10-05T16:22:11Z
    const T: f64 = 1_570_292_531.0;

    #[test]
    fn la_fecha_se_escribe_con_el_desfase_de_fotos() {
        assert_eq!(iso_utc(T), "2019-10-05T16:22:11Z");
        assert_eq!(fecha_local(T, 7_200), "2019-10-05T18:22:11+02:00");
        assert_eq!(fecha_local(T, -18_000), "2019-10-05T11:22:11-05:00");
        assert_eq!(fecha_local(T, 19_800), "2019-10-05T21:52:11+05:30");
        // Fotos guarda fracciones de segundo.
        assert_eq!(iso_utc(T + 0.9996), "2019-10-05T16:22:12Z");
        assert_eq!(iso_utc(T + 0.9994), "2019-10-05T16:22:11Z");
    }

    #[test]
    fn la_carpeta_cambia_de_mes_y_de_anno_con_el_desfase() {
        // 2019-12-31T23:30:00Z
        let fin = 1_577_835_000.0;
        assert_eq!(carpeta(fin, 0), "2019/2019-12");
        assert_eq!(carpeta(fin, 3_600), "2020/2020-01");
        assert_eq!(carpeta(-1.0, 0), "1969/1969-12");
        // 29 de febrero de un año bisiesto: 2024-02-29T12:00:00Z
        let c = civil(1_709_208_000.0, 0);
        assert_eq!((c.anno, c.mes, c.dia), (2024, 2, 29));
    }

    #[test]
    fn las_coordenadas_van_en_grados_y_minutos() {
        assert_eq!(coordenada(41.5, 'N', 'S'), "41,30.000000N");
        assert_eq!(coordenada(-0.25, 'E', 'W'), "0,15.000000W");
    }

    #[test]
    fn sin_nada_que_decir_no_hay_sidecar() {
        assert!(sidecar(&Meta::default(), false, Some(T), None).is_none());
    }

    #[test]
    fn el_sidecar_lleva_favorito_personas_fecha_y_lugar() {
        let meta = Meta {
            descripcion: Some("Tom & Ana <3".into()),
            palabras_clave: vec!["playa".into()],
            personas: vec!["Ana".into()],
            desfase: Some(7_200),
            ..Meta::default()
        };
        let x = sidecar(&meta, true, Some(T), Some((41.5, -0.25))).unwrap();
        assert!(x.contains(MARCA));
        assert!(x.contains("<rdf:li>Favorito</rdf:li><rdf:li>playa</rdf:li><rdf:li>Ana</rdf:li>"));
        assert!(x.contains("<rdf:li>Personas|Ana</rdf:li>"));
        assert!(x.contains("Tom &amp; Ana &lt;3"));
        assert!(
            x.contains("<exif:DateTimeOriginal>2019-10-05T18:22:11+02:00</exif:DateTimeOriginal>")
        );
        assert!(x.contains("<exif:GPSLatitude>41,30.000000N</exif:GPSLatitude>"));
        assert!(x.contains("<exif:GPSLongitude>0,15.000000W</exif:GPSLongitude>"));
    }
}
