import SwiftUI
import UIKit

@_silgen_name("perry_main_init") func perry_main_init()
@_silgen_name("perry_visionos_root_view") func perry_visionos_root_view() -> Int64

final class PerryVisionBootstrap {
    static let shared = PerryVisionBootstrap()
    let rootView: UIView

    private init() {
        perry_main_init()
        let ptr = perry_visionos_root_view()
        if ptr != 0 {
            rootView = Unmanaged<UIView>.fromOpaque(UnsafeRawPointer(bitPattern: UInt(ptr))!).takeRetainedValue()
        } else {
            let label = UILabel()
            label.text = "Perry visionOS root view missing"
            rootView = label
        }
    }
}

struct PerryHostedView: UIViewRepresentable {
    func makeUIView(context: Context) -> UIView {
        let container = UIView(frame: .zero)
        let root = PerryVisionBootstrap.shared.rootView
        root.translatesAutoresizingMaskIntoConstraints = false
        container.addSubview(root)
        NSLayoutConstraint.activate([
            root.leadingAnchor.constraint(equalTo: container.leadingAnchor),
            root.trailingAnchor.constraint(equalTo: container.trailingAnchor),
            root.topAnchor.constraint(equalTo: container.topAnchor),
            root.bottomAnchor.constraint(equalTo: container.bottomAnchor),
        ])

        let env = ProcessInfo.processInfo.environment
        if let screenshotPath = env["PERRY_UI_SCREENSHOT_PATH"],
           let testMode = env["PERRY_UI_TEST_MODE"],
           !testMode.isEmpty,
           testMode != "0",
           !testMode.lowercased().elementsEqual("false") {
            let delayMs = Int(env["PERRY_UI_TEST_EXIT_AFTER_MS"] ?? "1000") ?? 1000
            DispatchQueue.main.asyncAfter(deadline: .now() + .milliseconds(delayMs)) {
                let resolvedPath: String
                if screenshotPath.hasPrefix("/") {
                    resolvedPath = screenshotPath
                } else {
                    resolvedPath = "\(NSHomeDirectory())/\(screenshotPath)"
                }
                let url = URL(fileURLWithPath: resolvedPath)
                let dir = url.deletingLastPathComponent()
                try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
                var bounds = container.bounds
                if bounds.width < 2 || bounds.height < 2 {
                    bounds = CGRect(x: 0, y: 0, width: 1280, height: 720)
                    container.frame = bounds
                    root.frame = container.bounds
                    container.layoutIfNeeded()
                }
                let renderer = UIGraphicsImageRenderer(bounds: bounds)
                let image = renderer.image { ctx in
                    container.drawHierarchy(in: bounds, afterScreenUpdates: true)
                }
                if let data = image.pngData() {
                    try? data.write(to: url)
                }
                exit(0)
            }
        }

        return container
    }

    func updateUIView(_ uiView: UIView, context: Context) {}
}

@main
struct PerryVisionApp: App {
    init() {
        _ = PerryVisionBootstrap.shared
    }

    var body: some Scene {
        WindowGroup {
            PerryHostedView()
        }
    }
}
