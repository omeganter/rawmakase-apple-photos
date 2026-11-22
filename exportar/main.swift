// fotos-a-rawmakase · exportador
// Copia la fototeca de Fotos (macOS) a una carpeta normal, sin modificar la fototeca:
//   AAAA/AAAA-MM/IMG_0001.HEIC          original, byte a byte
//   AAAA/AAAA-MM/IMG_0001_editada.jpg   versión editada tal como la muestra Fotos (si la hay)
//   AAAA/AAAA-MM/IMG_0001.HEIC.xmp      título, descripción, palabras clave, personas, "Favorito",
//                                       y la fecha y el lugar tal como los muestra Fotos
//   AAAA/AAAA-MM/IMG_0001.MOV           vídeo de la Live Photo (con --videos)
//   _fotos/ajustes/<id>.plist           datos del ajuste de Apple, por si se pueden convertir más adelante
//   _fotos/manifiesto.json              fotos, álbumes y carpetas, con su orden
//
// Uso (dentro del .app, por el permiso de Fotos):
//   open -n FotosARawmakase.app --args --dest /ruta/destino [--album "Nombre"] [--limit N] [--videos] [--rafagas] [--ocultas] [--dry-run] [--solo-editadas] [--solo-con-metadatos]
import Foundation
import Photos
import SQLite3
import CoreLocation

// MARK: - Opciones

var dest = ""
var albumFilter: String?
var limit: Int?
var includeVideos = false
var dryRun = false
var onlyEdited = false  // para pruebas: solo fotos con ediciones
var includeBursts = false  // todas las fotos de cada ráfaga, no solo la elegida
var includeHidden = false  // también las del álbum Oculto
var onlyWithMeta = false  // para pruebas: solo fotos con título, descripción, palabras clave o personas

var args = Array(CommandLine.arguments.dropFirst())
while !args.isEmpty {
    let a = args.removeFirst()
    switch a {
    case "--dest": dest = args.isEmpty ? "" : args.removeFirst()
    case "--album": albumFilter = args.isEmpty ? nil : args.removeFirst()
    case "--limit": limit = args.isEmpty ? nil : Int(args.removeFirst())
    case "--videos": includeVideos = true
    case "--dry-run": dryRun = true
    case "--solo-editadas": onlyEdited = true
    case "--solo-con-metadatos": onlyWithMeta = true
    case "--rafagas": includeBursts = true
    case "--ocultas": includeHidden = true
    default: break
    }
}
guard !dest.isEmpty else {
    FileHandle.standardError.write("Falta --dest /ruta/destino\n".data(using: .utf8)!)
    exit(2)
}

let fm = FileManager.default
let destURL = URL(fileURLWithPath: dest, isDirectory: true)
let metaURL = destURL.appendingPathComponent("_fotos", isDirectory: true)
let adjURL = metaURL.appendingPathComponent("ajustes", isDirectory: true)
try? fm.createDirectory(at: adjURL, withIntermediateDirectories: true)
let logURL = metaURL.appendingPathComponent(dryRun ? "simulacion.log" : "exportacion.log")
fm.createFile(atPath: logURL.path, contents: nil)
let logHandle = try? FileHandle(forWritingTo: logURL)

func log(_ s: String) {
    let line = s + "\n"
    logHandle?.write(line.data(using: .utf8)!)
    FileHandle.standardOutput.write(line.data(using: .utf8)!)
}

func finish(_ code: Int32) -> Never {
    try? logHandle?.close()
    exit(code)
}

// MARK: - Utilidades

let folderFormatter: DateFormatter = {
    let f = DateFormatter()
    f.locale = Locale(identifier: "en_US_POSIX")
    f.dateFormat = "yyyy/yyyy-MM"
    return f
}()
let iso = ISO8601DateFormatter()

/// Nombre de archivo seguro también en exFAT/Windows.
func safeName(_ s: String) -> String {
    var out = s.map { ch -> Character in "/\\:*?\"<>|".contains(ch) || ch.isNewline ? "_" : ch }
    while let last = out.last, last == "." || last == " " { out.removeLast() }
    return out.isEmpty ? "sin-nombre" : String(out)
}

func shortID(_ localIdentifier: String) -> String {
    String(localIdentifier.prefix(8))
}

/// Escribe un recurso de Fotos en `url` (primero a un archivo parcial). Descarga de iCloud si hace falta.
func write(_ resource: PHAssetResource, to url: URL) -> Error? {
    let partial = url.appendingPathExtension("parcial")
    try? fm.removeItem(at: partial)
    let options = PHAssetResourceRequestOptions()
    options.isNetworkAccessAllowed = true
    let sem = DispatchSemaphore(value: 0)
    var failure: Error?
    PHAssetResourceManager.default().writeData(for: resource, toFile: partial, options: options) { error in
        failure = error
        sem.signal()
    }
    sem.wait()
    if let failure { try? fm.removeItem(at: partial); return failure }
    do { try fm.moveItem(at: partial, to: url) } catch { return error }
    return nil
}

func exists(_ url: URL) -> Bool {
    guard let size = (try? fm.attributesOfItem(atPath: url.path))?[.size] as? NSNumber else { return false }
    return size.intValue > 0
}

// MARK: - Sidecar XMP

func xmlEscape(_ s: String) -> String {
    s.replacingOccurrences(of: "&", with: "&amp;").replacingOccurrences(of: "<", with: "&lt;")
        .replacingOccurrences(of: ">", with: "&gt;")
}

/// Metadatos de una foto que no viajan dentro del archivo.
struct Meta {
    var title: String?
    var caption: String?
    var keywords: [String] = []
    var people: [String] = []
    /// Desfase horario (segundos) con el que Fotos muestra la fecha de la foto.
    var tzOffset: Int?
    var isEmpty: Bool { title == nil && caption == nil && keywords.isEmpty && people.isEmpty }
}

/// Fecha de captura como la muestra Fotos (también si se corrigió allí), con su desfase: 2019-10-05T18:22:11+02:00
func xmpDate(_ date: Date, offset: Int) -> String {
    let f = DateFormatter()
    f.locale = Locale(identifier: "en_US_POSIX")
    f.timeZone = TimeZone(secondsFromGMT: offset)
    f.dateFormat = "yyyy-MM-dd'T'HH:mm:ss"
    let a = abs(offset)
    return f.string(from: date) + String(format: "%@%02d:%02d", offset < 0 ? "-" : "+", a / 3600, (a % 3600) / 60)
}

/// Coordenada en el formato de XMP: "41,23.4567N".
func xmpCoordinate(_ value: Double, positive: String, negative: String) -> String {
    let a = abs(value), degrees = Int(a)
    return String(format: "%d,%.6f%@", degrees, (a - Double(degrees)) * 60, value < 0 ? negative : positive)
}

let sidecarMarker = "<!-- fotos-a-rawmakase -->"

/// Sidecar que RAWmakase lee al añadir la carpeta: título, descripción y palabras clave.
/// Las personas van como palabras clave bajo "Personas".
func sidecar(_ meta: Meta, favorite: Bool, date: Date?, location: CLLocation?) -> String? {
    var flat = meta.keywords + meta.people
    if favorite { flat.insert("Favorito", at: 0) }
    let when = (date != nil && meta.tzOffset != nil) ? xmpDate(date!, offset: meta.tzOffset!) : nil
    if flat.isEmpty && meta.title == nil && meta.caption == nil && when == nil && location == nil { return nil }
    func bag(_ items: [String]) -> String { items.map { "<rdf:li>\(xmlEscape($0))</rdf:li>" }.joined() }
    var body = ""
    if let t = meta.title { body += "   <dc:title><rdf:Alt><rdf:li xml:lang=\"x-default\">\(xmlEscape(t))</rdf:li></rdf:Alt></dc:title>\n" }
    if let c = meta.caption { body += "   <dc:description><rdf:Alt><rdf:li xml:lang=\"x-default\">\(xmlEscape(c))</rdf:li></rdf:Alt></dc:description>\n" }
    if !flat.isEmpty { body += "   <dc:subject><rdf:Bag>\(bag(flat))</rdf:Bag></dc:subject>\n" }
    if !meta.people.isEmpty {
        body += "   <lr:hierarchicalSubject><rdf:Bag>\(bag(meta.people.map { "Personas|\($0)" }))</rdf:Bag></lr:hierarchicalSubject>\n"
    }
    if let when { body += "   <exif:DateTimeOriginal>\(when)</exif:DateTimeOriginal>\n" }
    if let c = location?.coordinate, CLLocationCoordinate2DIsValid(c) {
        body += "   <exif:GPSLatitude>\(xmpCoordinate(c.latitude, positive: "N", negative: "S"))</exif:GPSLatitude>\n"
        body += "   <exif:GPSLongitude>\(xmpCoordinate(c.longitude, positive: "E", negative: "W"))</exif:GPSLongitude>\n"
    }
    return """
    <?xpacket begin="" id="W5M0MpCehiHzreSzNTczkc9d"?>
    \(sidecarMarker)
    <x:xmpmeta xmlns:x="adobe:ns:meta/">
     <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
      <rdf:Description rdf:about="" xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:lr="http://ns.adobe.com/lightroom/1.0/" xmlns:exif="http://ns.adobe.com/exif/1.0/">
    \(body)  </rdf:Description>
     </rdf:RDF>
    </x:xmpmeta>
    <?xpacket end="w"?>

    """
}

// MARK: - Base de datos de Fotos (Photos.sqlite)
//
// PhotoKit no da títulos, descripciones, palabras clave ni personas, así que se leen de la base de
// datos de la fototeca, en solo lectura. No es una interfaz oficial: Apple cambia tablas y columnas
// entre versiones de macOS. Por eso:
//   · las columnas que cambian de nombre se buscan en la propia base de datos, no se dan por sabidas;
//   · cada versión de macOS comprobada a mano se apunta en `macOSComprobados`;
//   · el manifiesto registra la versión de macOS, la del esquema y qué columnas se usaron, para que
//     quien lo importe sepa de dónde salen los datos y si esa versión estaba comprobada.
// Para dar por buena otra versión de macOS: exportar, revisar el bloque "origen" del manifiesto y
// unas cuantas fotos con metadatos, y añadir su número aquí.

/// Versiones mayores de macOS en las que se ha comprobado la lectura de Photos.sqlite.
let macOSComprobados: Set<Int> = [27]

struct LecturaBD {
    var metas: [String: Meta] = [:]  // por UUID de la foto
    var origen: [String: Any] = [:]  // lo que consta en el manifiesto
}

func leerBaseDeDatos() -> LecturaBD {
    var out = LecturaBD()
    let v = ProcessInfo.processInfo.operatingSystemVersion
    out.origen["macos"] = "\(v.majorVersion).\(v.minorVersion).\(v.patchVersion)"
    out.origen["macos_comprobado"] = macOSComprobados.contains(v.majorVersion)
    out.origen["metadatos"] = "no leídos"

    let path = NSHomeDirectory() + "/Pictures/Photos Library.photoslibrary/database/Photos.sqlite"
    var db: OpaquePointer?
    let uri = "file:" + path.addingPercentEncoding(withAllowedCharacters: .urlPathAllowed)! + "?mode=ro"
    guard sqlite3_open_v2(uri, &db, SQLITE_OPEN_READONLY | SQLITE_OPEN_URI, nil) == SQLITE_OK else {
        log("Aviso: no se puede abrir la base de datos de Fotos; se exporta sin títulos, descripciones, palabras clave ni personas.")
        sqlite3_close(db)
        return out
    }
    defer { sqlite3_close(db) }
    sqlite3_busy_timeout(db, 5000)

    func rows(_ sql: String) -> [[String?]]? {
        var st: OpaquePointer?
        guard sqlite3_prepare_v2(db, sql, -1, &st, nil) == SQLITE_OK else { return nil }
        defer { sqlite3_finalize(st) }
        var result: [[String?]] = []
        while sqlite3_step(st) == SQLITE_ROW {
            result.append((0..<sqlite3_column_count(st)).map { i in sqlite3_column_text(st, i).map { String(cString: $0) } })
        }
        return result
    }
    func columns(_ table: String) -> [String] {
        (rows("SELECT name FROM pragma_table_info('\(table)')") ?? []).compactMap { $0[0] }
    }
    func tables(like pattern: String) -> [String] {
        (rows("SELECT name FROM sqlite_master WHERE type='table' AND name LIKE '\(pattern)' ESCAPE '\\' ORDER BY name") ?? []).compactMap { $0[0] }
    }

    var esquema: [String: Any] = [:]
    if let z = rows("SELECT Z_VERSION FROM Z_METADATA")?.first?[0] { esquema["z_version"] = z }
    var usado: [String: String] = [:]
    var faltan: [String] = []

    // Títulos y descripciones
    let attr = columns("ZADDITIONALASSETATTRIBUTES")
    if attr.contains("ZASSET"), attr.contains("ZTITLE"), columns("ZASSET").contains("ZUUID") {
        for r in rows("SELECT a.ZUUID, x.ZTITLE FROM ZADDITIONALASSETATTRIBUTES x JOIN ZASSET a ON a.Z_PK = x.ZASSET WHERE x.ZTITLE IS NOT NULL AND x.ZTITLE <> ''") ?? [] {
            if let u = r[0], let t = r[1] { out.metas[u, default: Meta()].title = t }
        }
        usado["titulo"] = "ZADDITIONALASSETATTRIBUTES.ZTITLE"
    } else { faltan.append("titulo") }

    // Desfase horario de cada foto, para escribir la fecha tal como la muestra Fotos.
    if attr.contains("ZASSET"), attr.contains("ZTIMEZONEOFFSET") {
        for r in rows("SELECT a.ZUUID, x.ZTIMEZONEOFFSET FROM ZADDITIONALASSETATTRIBUTES x JOIN ZASSET a ON a.Z_PK = x.ZASSET WHERE x.ZTIMEZONEOFFSET IS NOT NULL") ?? [] {
            if let u = r[0], let t = r[1], let seconds = Int(t) { out.metas[u, default: Meta()].tzOffset = seconds }
        }
        usado["zona_horaria"] = "ZADDITIONALASSETATTRIBUTES.ZTIMEZONEOFFSET"
    } else { faltan.append("zona_horaria") }

    let desc = columns("ZASSETDESCRIPTION")
    if desc.contains("ZLONGDESCRIPTION"), desc.contains("ZASSETATTRIBUTES"), attr.contains("ZASSET") {
        for r in rows("SELECT a.ZUUID, d.ZLONGDESCRIPTION FROM ZASSETDESCRIPTION d JOIN ZADDITIONALASSETATTRIBUTES x ON x.Z_PK = d.ZASSETATTRIBUTES JOIN ZASSET a ON a.Z_PK = x.ZASSET WHERE d.ZLONGDESCRIPTION IS NOT NULL AND d.ZLONGDESCRIPTION <> ''") ?? [] {
            if let u = r[0], let c = r[1] { out.metas[u, default: Meta()].caption = c }
        }
        usado["descripcion"] = "ZASSETDESCRIPTION.ZLONGDESCRIPTION"
    } else { faltan.append("descripcion") }

    // Palabras clave: la tabla de unión se llama Z_<n>KEYWORDS y sus columnas llevan números que cambian.
    if let join = tables(like: "Z\\_%KEYWORDS").first {
        let cols = columns(join)
        if let cAttr = cols.first(where: { $0.hasSuffix("ASSETATTRIBUTES") }), let cKey = cols.first(where: { $0.hasSuffix("KEYWORDS") }),
           columns("ZKEYWORD").contains("ZTITLE") {
            for r in rows("SELECT a.ZUUID, k.ZTITLE FROM \(join) j JOIN ZKEYWORD k ON k.Z_PK = j.\(cKey) JOIN ZADDITIONALASSETATTRIBUTES x ON x.Z_PK = j.\(cAttr) JOIN ZASSET a ON a.Z_PK = x.ZASSET WHERE k.ZTITLE IS NOT NULL AND k.ZTITLE <> '' ORDER BY k.ZTITLE") ?? [] {
                if let u = r[0], let k = r[1] { out.metas[u, default: Meta()].keywords.append(k) }
            }
            usado["palabras_clave"] = "\(join).\(cAttr)/\(cKey)"
        } else { faltan.append("palabras_clave") }
    } else { faltan.append("palabras_clave") }

    // Personas con nombre: la columna de la cara hacia la foto y hacia la persona ha cambiado de nombre.
    let face = columns("ZDETECTEDFACE"), person = columns("ZPERSON")
    if let cAsset = ["ZASSETFORFACE", "ZASSET"].first(where: face.contains),
       let cPerson = ["ZPERSONFORFACE", "ZPERSON"].first(where: face.contains),
       let cName = ["ZFULLNAME", "ZDISPLAYNAME"].first(where: person.contains) {
        for r in rows("SELECT DISTINCT a.ZUUID, p.\(cName) FROM ZDETECTEDFACE f JOIN ZPERSON p ON p.Z_PK = f.\(cPerson) JOIN ZASSET a ON a.Z_PK = f.\(cAsset) WHERE p.\(cName) IS NOT NULL AND p.\(cName) <> '' ORDER BY p.\(cName)") ?? [] {
            if let u = r[0], let n = r[1] { out.metas[u, default: Meta()].people.append(n) }
        }
        usado["personas"] = "ZDETECTEDFACE.\(cAsset)/\(cPerson), ZPERSON.\(cName)"
    } else { faltan.append("personas") }

    esquema["columnas"] = usado
    if !faltan.isEmpty { esquema["no_encontrado"] = faltan }
    out.origen["esquema_fotos"] = esquema
    out.origen["metadatos"] = faltan.isEmpty ? "base de datos de Fotos" : "base de datos de Fotos (incompleto)"
    let n = out.metas.values
    log("Base de datos de Fotos · macOS \(out.origen["macos"]!)\(macOSComprobados.contains(v.majorVersion) ? "" : " (versión NO comprobada)") · con título \(n.filter { $0.title != nil }.count) · con descripción \(n.filter { $0.caption != nil }.count) · con palabras clave \(n.filter { !$0.keywords.isEmpty }.count) · con personas \(n.filter { !$0.people.isEmpty }.count) · con zona horaria \(n.filter { $0.tzOffset != nil }.count)\(faltan.isEmpty ? "" : " · no encontrado: \(faltan.joined(separator: ", "))")")
    return out
}

// MARK: - Exportación

func run() {
    let bd = leerBaseDeDatos()
    // 1. Carpetas y álbumes del usuario, con su jerarquía y el orden de sus fotos.
    var collections: [[String: Any]] = []
    var wanted = Set<String>()  // con --album: solo las fotos de esos álbumes
    func walk(_ list: PHFetchResult<PHCollection>, parent: String?) {
        list.enumerateObjects { c, _, _ in
            if let folder = c as? PHCollectionList {
                collections.append(["id": folder.localIdentifier, "nombre": folder.localizedTitle ?? "Sin título",
                                    "tipo": "carpeta", "padre": parent as Any? ?? NSNull()])
                walk(PHCollection.fetchCollections(in: folder, options: nil), parent: folder.localIdentifier)
            } else if let album = c as? PHAssetCollection {
                let title = album.localizedTitle ?? "Sin título"
                if let albumFilter, title != albumFilter { return }
                var ids: [String] = []
                PHAsset.fetchAssets(in: album, options: nil).enumerateObjects { a, _, _ in ids.append(a.localIdentifier) }
                wanted.formUnion(ids)
                collections.append(["id": album.localIdentifier, "nombre": title, "tipo": "album",
                                    "padre": parent as Any? ?? NSNull(), "fotos": ids])
            }
        }
    }
    walk(PHCollection.fetchTopLevelUserCollections(with: nil), parent: nil)
    if albumFilter != nil {
        // Con filtro, las carpetas vacías sobran.
        let parents = Set(collections.compactMap { $0["padre"] as? String })
        collections.removeAll { ($0["tipo"] as? String) == "carpeta" && !parents.contains($0["id"] as! String) }
    }

    // 2. Fotos a exportar, en orden estable (fecha y luego identificador).
    let options = PHFetchOptions()
    options.sortDescriptors = [NSSortDescriptor(key: "creationDate", ascending: true)]
    options.includeAllBurstAssets = includeBursts
    options.includeHiddenAssets = includeHidden
    var assets: [PHAsset] = []
    PHAsset.fetchAssets(with: options).enumerateObjects { a, _, _ in
        if albumFilter != nil && !wanted.contains(a.localIdentifier) { return }
        if a.mediaType == .video && !includeVideos { return }
        if a.mediaType != .image && a.mediaType != .video { return }
        if onlyWithMeta && (bd.metas[String(a.localIdentifier.prefix(36))]?.isEmpty ?? true) { return }
        if onlyEdited && !PHAssetResource.assetResources(for: a).contains(where: { $0.type == .adjustmentData }) { return }
        assets.append(a)
    }
    if let limit { assets = Array(assets.prefix(limit)) }
    log("Elementos a exportar: \(assets.count)\(dryRun ? " (simulación, no se copia nada)" : "")")

    var used = Set<String>()  // rutas relativas ya asignadas, en minúsculas
    var manifest: [[String: Any]] = []
    var copied = 0, present = 0, failed = 0, editedCount = 0
    var bytes: Int64 = 0

    /// Asigna una ruta relativa única y estable para un archivo.
    func assign(_ folder: String, _ base: String, _ ext: String, _ id: String) -> String {
        var rel = "\(folder)/\(base)\(ext.isEmpty ? "" : ".\(ext)")"
        if used.contains(rel.lowercased()) {
            rel = "\(folder)/\(base)_\(shortID(id))\(ext.isEmpty ? "" : ".\(ext)")"
        }
        used.insert(rel.lowercased())
        return rel
    }

    /// Copia un recurso si aún no está. Devuelve false si falla.
    func export(_ resource: PHAssetResource, _ rel: String) -> Bool {
        let url = destURL.appendingPathComponent(rel)
        if exists(url) { present += 1; return true }
        if dryRun { copied += 1; return true }
        try? fm.createDirectory(at: url.deletingLastPathComponent(), withIntermediateDirectories: true)
        if let error = write(resource, to: url) {
            failed += 1
            log("ERROR \(rel): \(error.localizedDescription)")
            return false
        }
        copied += 1
        bytes += ((try? fm.attributesOfItem(atPath: url.path))?[.size] as? NSNumber)?.int64Value ?? 0
        return true
    }

    let cancelURL = metaURL.appendingPathComponent("cancelar")
    for (index, asset) in assets.enumerated() {
        // Quien lanzó la exportación puede pedir que pare creando este archivo. Lo copiado se queda.
        if fm.fileExists(atPath: cancelURL.path) {
            try? fm.removeItem(at: cancelURL)
            log("CANCELADO · \(index)/\(assets.count) · copiados \(copied) · ya estaban \(present) · errores \(failed)")
            finish(5)
        }
        let id = asset.localIdentifier
        let resources = PHAssetResource.assetResources(for: asset)
        let isVideo = asset.mediaType == .video
        guard let original = resources.first(where: { $0.type == (isVideo ? .video : .photo) }) else {
            failed += 1
            log("ERROR \(id): sin recurso original")
            continue
        }
        let folder = asset.creationDate.map { folderFormatter.string(from: $0) } ?? "sin-fecha"
        let name = safeName(original.originalFilename) as NSString
        let base = name.deletingPathExtension, ext = name.pathExtension

        var entry: [String: Any] = ["id": id, "tipo": isVideo ? "video" : "foto", "favorito": asset.isFavorite]
        if let d = asset.creationDate { entry["fecha"] = iso.string(from: d) }

        let relOriginal = assign(folder, base, ext, id)
        if export(original, relOriginal) { entry["original"] = relOriginal }
        let stem = (relOriginal as NSString).deletingPathExtension  // con el sufijo de colisión, si lo hubo

        // RAW de una pareja RAW+JPEG
        if let raw = resources.first(where: { $0.type == .alternatePhoto }) {
            let rawExt = (raw.originalFilename as NSString).pathExtension
            let rel = assign(folder, (stem as NSString).lastPathComponent, rawExt, id)
            if export(raw, rel) { entry["raw"] = rel }
        }

        // Versión editada, tal como la muestra Fotos
        if let edited = resources.first(where: { $0.type == (isVideo ? .fullSizeVideo : .fullSizePhoto) }) {
            let editedExt = (edited.originalFilename as NSString).pathExtension
            let rel = assign(folder, (stem as NSString).lastPathComponent + "_editada", editedExt.isEmpty ? "jpg" : editedExt, id)
            if export(edited, rel) { entry["editada"] = rel; editedCount += 1 }
        }

        // Vídeo de una Live Photo (solo con --videos), con el mismo nombre que su foto
        if includeVideos && !isVideo {
            if let paired = resources.first(where: { $0.type == .pairedVideo }) {
                let e = (paired.originalFilename as NSString).pathExtension
                let rel = assign(folder, (stem as NSString).lastPathComponent, e.isEmpty ? "mov" : e, id)
                if export(paired, rel) { entry["video_live"] = rel }
            }
            if let paired = resources.first(where: { $0.type == .fullSizePairedVideo }) {
                let e = (paired.originalFilename as NSString).pathExtension
                let rel = assign(folder, (stem as NSString).lastPathComponent + "_editada", e.isEmpty ? "mov" : e, id)
                if export(paired, rel) { entry["video_live_editado"] = rel }
            }
        }
        if asset.mediaSubtypes.contains(.photoLive) { entry["live"] = true }
        if let burst = asset.burstIdentifier { entry["rafaga"] = burst }
        if asset.isHidden { entry["oculta"] = true }
        if let c = asset.location?.coordinate, CLLocationCoordinate2DIsValid(c) { entry["lugar"] = [c.latitude, c.longitude] }
        if let d = asset.creationDate, let o = (bd.metas[String(id.prefix(36))]?.tzOffset) { entry["fecha_local"] = xmpDate(d, offset: o) }

        // Datos del ajuste de Apple (no los lee RAWmakase; se guardan para el futuro)
        if let adjustment = resources.first(where: { $0.type == .adjustmentData }) {
            let rel = "_fotos/ajustes/\(safeName(id)).plist"
            let url = destURL.appendingPathComponent(rel)
            if exists(url) { entry["ajuste"] = rel } else if !dryRun, write(adjustment, to: url) == nil { entry["ajuste"] = rel }
        }

        // Lo que no viaja dentro del archivo, en un sidecar XMP que RAWmakase lee al añadir la carpeta.
        // "IMG_0001.JPG.xmp" vale para cualquier formato; "IMG_0001.xmp" solo lo lee para los RAW.
        let meta = bd.metas[String(id.prefix(36))] ?? Meta()
        if let t = meta.title { entry["titulo"] = t }
        if let c = meta.caption { entry["descripcion"] = c }
        if !meta.keywords.isEmpty { entry["palabras_clave"] = meta.keywords }
        if !meta.people.isEmpty { entry["personas"] = meta.people }
        if !dryRun, let xmpText = sidecar(meta, favorite: asset.isFavorite, date: asset.creationDate, location: asset.location) {
            for key in ["original", "raw", "editada"] {
                guard let rel = entry[key] as? String else { continue }
                let xmp = destURL.appendingPathComponent(rel + ".xmp")
                // Nunca se pisa un sidecar que no sea nuestro.
                let previous = try? String(contentsOf: xmp, encoding: .utf8)
                if previous == nil || previous!.contains(sidecarMarker) { try? xmpText.write(to: xmp, atomically: true, encoding: .utf8) }
            }
        }

        manifest.append(entry)
        if (index + 1) % 100 == 0 || index + 1 == assets.count {
            log("\(index + 1)/\(assets.count) · copiados \(copied) · ya estaban \(present) · errores \(failed)")
        }
    }

    // 3. Manifiesto
    let doc: [String: Any] = [
        "version": 2,
        "herramienta": "fotos-a-rawmakase 0.3",
        "generado": iso.string(from: Date()),
        "origen": bd.origen,
        "opciones": ["videos": includeVideos, "rafagas": includeBursts, "ocultas": includeHidden,
                     "album": albumFilter as Any? ?? NSNull()],
        "destino": destURL.path,
        "fotos": manifest,
        "colecciones": collections,
    ]
    if let data = try? JSONSerialization.data(withJSONObject: doc, options: [.prettyPrinted, .sortedKeys]) {
        let name = dryRun ? "manifiesto-simulacion.json" : "manifiesto.json"
        try? data.write(to: metaURL.appendingPathComponent(name), options: .atomic)
    }
    let albums = collections.filter { ($0["tipo"] as? String) == "album" }.count
    log("FIN · elementos \(assets.count) · archivos copiados \(copied) · ya estaban \(present) · editadas \(editedCount) · errores \(failed) · \(bytes / 1_000_000) MB · álbumes \(albums)")
    finish(failed == 0 ? 0 : 1)
}

PHPhotoLibrary.requestAuthorization(for: .readWrite) { status in
    guard status == .authorized else {
        log("Sin permiso de acceso completo a Fotos (estado \(status.rawValue)). Actívalo en Ajustes del Sistema > Privacidad y seguridad > Fotos.")
        finish(3)
    }
    DispatchQueue.global(qos: .userInitiated).async { run() }
}
dispatchMain()
