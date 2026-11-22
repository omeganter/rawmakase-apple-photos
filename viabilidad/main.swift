// Prueba de viabilidad: ¿puede un programa auxiliar leer la fototeca de Fotos con PhotoKit?
// Solo lee. Escribe un resumen (recuentos, sin fotos) en el archivo dado como primer argumento.
import Foundation
import Photos

let outPath = CommandLine.arguments.count > 1 ? CommandLine.arguments[1] : "/dev/stdout"
var report: [String: Any] = ["pid": getpid(), "started": ISO8601DateFormatter().string(from: Date())]

func finish() -> Never {
    let data = try! JSONSerialization.data(withJSONObject: report, options: [.prettyPrinted, .sortedKeys])
    FileManager.default.createFile(atPath: outPath, contents: data)
    exit(0)
}

func statusName(_ s: PHAuthorizationStatus) -> String {
    switch s {
    case .notDetermined: return "notDetermined"
    case .restricted: return "restricted"
    case .denied: return "denied"
    case .authorized: return "authorized"
    case .limited: return "limited"
    @unknown default: return "unknown"
    }
}

report["status_before"] = statusName(PHPhotoLibrary.authorizationStatus(for: .readWrite))

PHPhotoLibrary.requestAuthorization(for: .readWrite) { status in
    report["status_after"] = statusName(status)
    guard status == .authorized || status == .limited else { finish() }

    // Fotos y vídeos
    let all = PHAsset.fetchAssets(with: nil)
    var images = 0, videos = 0, edited = 0, favorites = 0, live = 0, raw = 0
    var resourceTypes: [String: Int] = [:]
    var sampleEdited: [[String: Any]] = []
    all.enumerateObjects { asset, _, _ in
        if asset.mediaType == .image { images += 1 } else if asset.mediaType == .video { videos += 1 }
        if asset.isFavorite { favorites += 1 }
        if asset.mediaSubtypes.contains(.photoLive) { live += 1 }
        let res = PHAssetResource.assetResources(for: asset)
        let hasAdj = res.contains { $0.type == .adjustmentData }
        if res.contains(where: { $0.type == .alternatePhoto }) { raw += 1 }
        if hasAdj {
            edited += 1
            if sampleEdited.count < 3 {
                sampleEdited.append([
                    "resources": res.map { "\($0.type.rawValue):\($0.uniformTypeIdentifier)" },
                    "pixel": "\(asset.pixelWidth)x\(asset.pixelHeight)",
                ])
            }
        }
        for r in res { resourceTypes["type_\(r.type.rawValue)", default: 0] += 1 }
    }
    report["assets"] = ["total": all.count, "images": images, "videos": videos, "edited": edited,
                        "favorites": favorites, "live_photos": live, "raw_plus_jpeg": raw]
    report["resource_types"] = resourceTypes
    report["sample_edited"] = sampleEdited

    // Carpetas y álbumes del usuario, con su jerarquía y orden
    var albums = 0, folders = 0, maxDepth = 0, albumAssets = 0
    var sampleTree: [String] = []
    func walk(_ list: PHFetchResult<PHCollection>, depth: Int) {
        maxDepth = max(maxDepth, depth)
        list.enumerateObjects { c, _, _ in
            if let folder = c as? PHCollectionList {
                folders += 1
                if sampleTree.count < 8 { sampleTree.append(String(repeating: "  ", count: depth) + "[carpeta] " + (folder.localizedTitle ?? "?")) }
                walk(PHCollection.fetchCollections(in: folder, options: nil), depth: depth + 1)
            } else if let album = c as? PHAssetCollection {
                albums += 1
                let n = PHAsset.fetchAssets(in: album, options: nil).count
                albumAssets += n
                if sampleTree.count < 8 { sampleTree.append(String(repeating: "  ", count: depth) + "[álbum] " + (album.localizedTitle ?? "?") + " (\(n))") }
            }
        }
    }
    walk(PHCollection.fetchTopLevelUserCollections(with: nil), depth: 0)
    let smart = PHAssetCollection.fetchAssetCollections(with: .smartAlbum, subtype: .any, options: nil).count
    report["collections"] = ["albums": albums, "folders": folders, "max_depth": maxDepth,
                             "album_memberships": albumAssets, "smart_albums_system": smart]
    report["sample_tree"] = sampleTree
    finish()
}

DispatchQueue.main.asyncAfter(deadline: .now() + 180) {
    report["error"] = "sin respuesta al aviso de permiso en 180 s"
    finish()
}
dispatchMain()
