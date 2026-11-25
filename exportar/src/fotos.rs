//! Lo que se le pide a Fotos, por PhotoKit (la interfaz pública de Apple).
//!
//! Todo el código `unsafe` del programa está aquí: son llamadas a métodos de Objective-C con los
//! argumentos que su documentación pide. El resto del programa solo ve tipos de Rust.
//! La fototeca solo se lee; ninguna llamada de este archivo la modifica.

use std::path::Path;
use std::sync::mpsc;

use block2::RcBlock;
use objc2::rc::Retained;
use objc2_foundation::{
    NSArray, NSError, NSProcessInfo, NSSortDescriptor, NSString, NSTimeZone, NSURL,
};
use objc2_photos::{
    PHAccessLevel, PHAsset, PHAssetCollection, PHAssetMediaSubtype, PHAssetMediaType,
    PHAssetResource, PHAssetResourceManager, PHAssetResourceRequestOptions, PHAssetResourceType,
    PHAuthorizationStatus, PHCollection, PHCollectionList, PHFetchOptions, PHFetchResult,
    PHPhotoLibrary,
};

/// Pide acceso completo a Fotos y espera la respuesta. `Err` lleva el estado que dio macOS.
pub fn pedir_permiso() -> Result<(), isize> {
    let (tx, rx) = mpsc::channel();
    let respuesta = RcBlock::new(move |estado: PHAuthorizationStatus| {
        let _ = tx.send(estado);
    });
    // SAFETY: el bloque vive hasta que macOS lo llama; solo envía un valor por el canal.
    unsafe {
        PHPhotoLibrary::requestAuthorizationForAccessLevel_handler(
            PHAccessLevel::ReadWrite,
            &respuesta,
        )
    };
    match rx.recv() {
        Ok(estado) if estado == PHAuthorizationStatus::Authorized => Ok(()),
        Ok(estado) => Err(estado.0),
        Err(_) => Err(-1),
    }
}

/// Versión de macOS: (mayor, menor, parche).
pub fn version_de_macos() -> (i64, i64, i64) {
    let v = NSProcessInfo::processInfo().operatingSystemVersion();
    (
        v.majorVersion as i64,
        v.minorVersion as i64,
        v.patchVersion as i64,
    )
}

/// Una carpeta o un álbum del usuario, con sus hijos en el orden de Fotos.
pub enum Coleccion {
    Carpeta {
        id: String,
        nombre: String,
        hijos: Vec<Coleccion>,
    },
    Album {
        id: String,
        nombre: String,
        fotos: Vec<String>,
    },
}

fn elementos<T: objc2::Message>(lista: &PHFetchResult<T>) -> Vec<Retained<T>> {
    // SAFETY: los índices están dentro de `count`.
    unsafe { (0..lista.count()).map(|i| lista.objectAtIndex(i)).collect() }
}

fn titulo(c: &PHCollection) -> String {
    // SAFETY: lectura de una propiedad.
    unsafe { c.localizedTitle() }.map_or_else(|| "Sin título".to_string(), |t| t.to_string())
}

fn recorrer(lista: &PHFetchResult<PHCollection>) -> Vec<Coleccion> {
    let mut salida = Vec::new();
    for c in elementos(lista) {
        // SAFETY: lecturas de propiedades y consultas a la fototeca.
        let id = unsafe { c.localIdentifier() }.to_string();
        let nombre = titulo(&c);
        match c.downcast::<PHCollectionList>() {
            Ok(carpeta) => {
                let dentro = unsafe {
                    PHCollection::fetchCollectionsInCollectionList_options(&carpeta, None)
                };
                salida.push(Coleccion::Carpeta {
                    id,
                    nombre,
                    hijos: recorrer(&dentro),
                });
            }
            Err(c) => {
                if let Ok(album) = c.downcast::<PHAssetCollection>() {
                    let dentro =
                        unsafe { PHAsset::fetchAssetsInAssetCollection_options(&album, None) };
                    let fotos = elementos(&dentro)
                        .iter()
                        .map(|a| unsafe { a.localIdentifier() }.to_string())
                        .collect();
                    salida.push(Coleccion::Album { id, nombre, fotos });
                }
            }
        }
    }
    salida
}

/// Carpetas y álbumes del usuario, con su jerarquía y el orden de sus fotos.
pub fn colecciones() -> Vec<Coleccion> {
    // SAFETY: consulta a la fototeca, sin opciones.
    let raiz = unsafe { PHCollection::fetchTopLevelUserCollectionsWithOptions(None) };
    recorrer(&raiz)
}

/// Una foto o un vídeo de la fototeca.
pub struct Elemento(Retained<PHAsset>);

/// Todas las fotos y vídeos, de la más antigua a la más reciente.
pub fn elementos_de_la_fototeca(rafagas: bool, ocultas: bool) -> Vec<Elemento> {
    // SAFETY: opciones recién creadas, con valores de los tipos que piden.
    let lista = unsafe {
        let opciones = PHFetchOptions::new();
        let orden = NSSortDescriptor::sortDescriptorWithKey_ascending(
            Some(&NSString::from_str("creationDate")),
            true,
        );
        opciones.setSortDescriptors(Some(&NSArray::from_retained_slice(&[orden])));
        opciones.setIncludeAllBurstAssets(rafagas);
        opciones.setIncludeHiddenAssets(ocultas);
        PHAsset::fetchAssetsWithOptions(Some(&opciones))
    };
    elementos(&lista).into_iter().map(Elemento).collect()
}

// SAFETY (todo el bloque): lecturas de propiedades de un PHAsset.
impl Elemento {
    pub fn id(&self) -> String {
        unsafe { self.0.localIdentifier() }.to_string()
    }
    pub fn es_video(&self) -> bool {
        unsafe { self.0.mediaType() == PHAssetMediaType::Video }
    }
    /// Ni foto ni vídeo (audio, desconocido).
    pub fn es_otro(&self) -> bool {
        let tipo = unsafe { self.0.mediaType() };
        tipo != PHAssetMediaType::Image && tipo != PHAssetMediaType::Video
    }
    /// Fecha de captura, en segundos desde 1970 (UTC).
    pub fn fecha(&self) -> Option<f64> {
        unsafe { self.0.creationDate() }.map(|d| d.timeIntervalSince1970())
    }
    /// Desfase (segundos) de la zona horaria de este Mac en la fecha de la foto: decide la
    /// carpeta AAAA/AAAA-MM, con el horario de verano de entonces.
    pub fn desfase_local(&self) -> i64 {
        unsafe { self.0.creationDate() }.map_or(0, |d| {
            NSTimeZone::localTimeZone().secondsFromGMTForDate(&d) as i64
        })
    }
    pub fn favorito(&self) -> bool {
        unsafe { self.0.isFavorite() }
    }
    pub fn oculta(&self) -> bool {
        unsafe { self.0.isHidden() }
    }
    pub fn live(&self) -> bool {
        unsafe { self.0.mediaSubtypes() }.contains(PHAssetMediaSubtype::PhotoLive)
    }
    pub fn rafaga(&self) -> Option<String> {
        unsafe { self.0.burstIdentifier() }.map(|b| b.to_string())
    }
    /// (latitud, longitud), si la foto tiene un lugar válido.
    pub fn lugar(&self) -> Option<(f64, f64)> {
        let c = unsafe { self.0.location()?.coordinate() };
        ((-90.0..=90.0).contains(&c.latitude) && (-180.0..=180.0).contains(&c.longitude))
            .then_some((c.latitude, c.longitude))
    }
    /// Los archivos que Fotos guarda de este elemento.
    pub fn recursos(&self) -> Vec<Recurso> {
        unsafe { PHAssetResource::assetResourcesForAsset(&self.0) }
            .to_vec()
            .into_iter()
            .map(Recurso)
            .collect()
    }
}

/// Qué es cada archivo de un elemento.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Clase {
    /// El original de una foto.
    Foto,
    /// El original de un vídeo.
    Video,
    /// El RAW de una pareja RAW+JPEG.
    Raw,
    /// La foto editada, tal como la muestra Fotos.
    FotoEditada,
    VideoEditado,
    /// Los datos del ajuste de Apple.
    Ajuste,
    /// El vídeo de una Live Photo.
    VideoLive,
    VideoLiveEditado,
    Otra,
}

/// Un archivo que Fotos guarda de un elemento.
pub struct Recurso(Retained<PHAssetResource>);

impl Recurso {
    pub fn clase(&self) -> Clase {
        // SAFETY: lectura de una propiedad.
        match unsafe { self.0.r#type() } {
            PHAssetResourceType::Photo => Clase::Foto,
            PHAssetResourceType::Video => Clase::Video,
            PHAssetResourceType::AlternatePhoto => Clase::Raw,
            PHAssetResourceType::FullSizePhoto => Clase::FotoEditada,
            PHAssetResourceType::FullSizeVideo => Clase::VideoEditado,
            PHAssetResourceType::AdjustmentData => Clase::Ajuste,
            PHAssetResourceType::PairedVideo => Clase::VideoLive,
            PHAssetResourceType::FullSizePairedVideo => Clase::VideoLiveEditado,
            _ => Clase::Otra,
        }
    }

    /// Nombre con el que se importó el archivo: "IMG_0001.HEIC".
    pub fn nombre(&self) -> String {
        // SAFETY: lectura de una propiedad.
        unsafe { self.0.originalFilename() }.to_string()
    }

    /// Escribe el archivo en `destino`, que no debe existir. Lo descarga de iCloud si hace falta.
    /// Espera a que Fotos termine; `Err` lleva el mensaje de Fotos.
    pub fn escribir(&self, destino: &Path) -> Result<(), String> {
        let (tx, rx) = mpsc::channel();
        let al_terminar = RcBlock::new(move |error: *mut NSError| {
            // SAFETY: Fotos pasa nulo o un NSError válido durante la llamada.
            let mensaje = unsafe { error.as_ref() }.map(|e| e.localizedDescription().to_string());
            let _ = tx.send(mensaje);
        });
        // SAFETY: argumentos de los tipos pedidos; el bloque vive hasta que Fotos lo llama.
        unsafe {
            let opciones = PHAssetResourceRequestOptions::new();
            opciones.setNetworkAccessAllowed(true);
            let url = NSURL::fileURLWithPath(&NSString::from_str(&destino.to_string_lossy()));
            PHAssetResourceManager::defaultManager()
                .writeDataForAssetResource_toFile_options_completionHandler(
                    &self.0,
                    &url,
                    Some(&opciones),
                    &al_terminar,
                );
        }
        match rx.recv() {
            Ok(None) => Ok(()),
            Ok(Some(mensaje)) => Err(mensaje),
            Err(_) => Err("Fotos no respondió".into()),
        }
    }
}
