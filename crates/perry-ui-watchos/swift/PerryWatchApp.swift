// Perry watchOS Runtime — fixed SwiftUI renderer
// Auto-shipped with Perry compiler. DO NOT EDIT.
//
// Queries a native UI tree (built by Cranelift-compiled TypeScript code)
// via FFI and renders it as SwiftUI views reactively.

import SwiftUI
import MapKit
import WatchKit

// MARK: - FFI declarations

@_silgen_name("perry_main_init") func perry_main_init()
// perry/background (#538) — WKApplicationDelegate.handle(_:) routes
// each delivered WKRefreshBackgroundTask's `userInfo["perry_id"]`
// here so Rust can dispatch the right registered handler.
@_silgen_name("perry_watchos_dispatch_background_task")
func perry_watchos_dispatch_background_task(_ id: UnsafePointer<CChar>)

// Timer ticks — drive setTimeout/setInterval from the JS runtime. Without
// these the TS event loop never advances on watchOS (there is no
// CADisplayLink here; the render Timer below is the only recurring host
// callback).
@_silgen_name("js_callback_timer_tick") func js_callback_timer_tick() -> Int32
@_silgen_name("js_interval_timer_tick") func js_interval_timer_tick() -> Int32

// Tree query
@_silgen_name("perry_watchos_root_node") func perry_watchos_root_node() -> Int64
@_silgen_name("perry_watchos_tree_version") func perry_watchos_tree_version() -> UInt64
@_silgen_name("perry_watchos_node_kind") func perry_watchos_node_kind(_ id: Int64) -> Int32
@_silgen_name("perry_watchos_node_text") func perry_watchos_node_text(_ id: Int64) -> UnsafePointer<CChar>?
@_silgen_name("perry_watchos_node_child_count") func perry_watchos_node_child_count(_ id: Int64) -> Int32
@_silgen_name("perry_watchos_node_child") func perry_watchos_node_child(_ id: Int64, _ idx: Int32) -> Int64
@_silgen_name("perry_watchos_node_hidden") func perry_watchos_node_hidden(_ id: Int64) -> Bool
@_silgen_name("perry_watchos_node_enabled") func perry_watchos_node_enabled(_ id: Int64) -> Bool
@_silgen_name("perry_watchos_node_opacity") func perry_watchos_node_opacity(_ id: Int64) -> Double
@_silgen_name("perry_watchos_node_spacing") func perry_watchos_node_spacing(_ id: Int64) -> Double

// Actions
@_silgen_name("perry_watchos_node_has_action") func perry_watchos_node_has_action(_ id: Int64) -> Bool
@_silgen_name("perry_watchos_handle_action") func perry_watchos_handle_action(_ id: Int64)

// Style
@_silgen_name("perry_watchos_node_font_size") func perry_watchos_node_font_size(_ id: Int64) -> Double
@_silgen_name("perry_watchos_node_font_weight") func perry_watchos_node_font_weight(_ id: Int64) -> Double
@_silgen_name("perry_watchos_node_has_color") func perry_watchos_node_has_color(_ id: Int64) -> Bool
@_silgen_name("perry_watchos_node_color") func perry_watchos_node_color(_ id: Int64, _ c: Int32) -> Double
@_silgen_name("perry_watchos_node_has_bg_color") func perry_watchos_node_has_bg_color(_ id: Int64) -> Bool
@_silgen_name("perry_watchos_node_bg_color") func perry_watchos_node_bg_color(_ id: Int64, _ c: Int32) -> Double
@_silgen_name("perry_watchos_node_corner_radius") func perry_watchos_node_corner_radius(_ id: Int64) -> Double
@_silgen_name("perry_watchos_node_frame_width") func perry_watchos_node_frame_width(_ id: Int64) -> Double
@_silgen_name("perry_watchos_node_frame_height") func perry_watchos_node_frame_height(_ id: Int64) -> Double
@_silgen_name("perry_watchos_node_padding") func perry_watchos_node_padding(_ id: Int64) -> Double
@_silgen_name("perry_watchos_node_text_wraps") func perry_watchos_node_text_wraps(_ id: Int64) -> Bool

// Slider
@_silgen_name("perry_watchos_node_slider_value") func perry_watchos_node_slider_value(_ id: Int64) -> Double
@_silgen_name("perry_watchos_node_slider_min") func perry_watchos_node_slider_min(_ id: Int64) -> Double
@_silgen_name("perry_watchos_node_slider_max") func perry_watchos_node_slider_max(_ id: Int64) -> Double
@_silgen_name("perry_watchos_slider_changed") func perry_watchos_slider_changed(_ id: Int64, _ value: Double)

// Toggle
@_silgen_name("perry_watchos_node_toggle_on") func perry_watchos_node_toggle_on(_ id: Int64) -> Bool
@_silgen_name("perry_watchos_toggle_changed") func perry_watchos_toggle_changed(_ id: Int64, _ on: Bool)

// ProgressView
@_silgen_name("perry_watchos_node_progress_value") func perry_watchos_node_progress_value(_ id: Int64) -> Double

// Picker
@_silgen_name("perry_watchos_node_picker_count") func perry_watchos_node_picker_count(_ id: Int64) -> Int32
@_silgen_name("perry_watchos_node_picker_item") func perry_watchos_node_picker_item(_ id: Int64, _ idx: Int32) -> UnsafePointer<CChar>?
@_silgen_name("perry_watchos_node_picker_selected") func perry_watchos_node_picker_selected(_ id: Int64) -> Int64
@_silgen_name("perry_watchos_picker_changed") func perry_watchos_picker_changed(_ id: Int64, _ idx: Int64)

// Image
@_silgen_name("perry_watchos_node_image_width") func perry_watchos_node_image_width(_ id: Int64) -> Double
@_silgen_name("perry_watchos_node_image_height") func perry_watchos_node_image_height(_ id: Int64) -> Double
@_silgen_name("perry_watchos_node_has_image_tint") func perry_watchos_node_has_image_tint(_ id: Int64) -> Bool
@_silgen_name("perry_watchos_node_image_tint") func perry_watchos_node_image_tint(_ id: Int64, _ c: Int32) -> Double

// Edge insets
@_silgen_name("perry_watchos_node_has_edge_insets") func perry_watchos_node_has_edge_insets(_ id: Int64) -> Bool
@_silgen_name("perry_watchos_node_edge_inset") func perry_watchos_node_edge_inset(_ id: Int64, _ side: Int32) -> Double

// MapView (issue #517)
@_silgen_name("perry_watchos_node_map_lat") func perry_watchos_node_map_lat(_ id: Int64) -> Double
@_silgen_name("perry_watchos_node_map_lon") func perry_watchos_node_map_lon(_ id: Int64) -> Double
@_silgen_name("perry_watchos_node_map_lat_span") func perry_watchos_node_map_lat_span(_ id: Int64) -> Double
@_silgen_name("perry_watchos_node_map_lon_span") func perry_watchos_node_map_lon_span(_ id: Int64) -> Double
@_silgen_name("perry_watchos_node_map_type") func perry_watchos_node_map_type(_ id: Int64) -> Int64
@_silgen_name("perry_watchos_node_map_pin_count") func perry_watchos_node_map_pin_count(_ id: Int64) -> Int32
@_silgen_name("perry_watchos_node_map_pin_lat") func perry_watchos_node_map_pin_lat(_ id: Int64, _ idx: Int32) -> Double
@_silgen_name("perry_watchos_node_map_pin_lon") func perry_watchos_node_map_pin_lon(_ id: Int64, _ idx: Int32) -> Double
@_silgen_name("perry_watchos_node_map_pin_title") func perry_watchos_node_map_pin_title(_ id: Int64, _ idx: Int32) -> UnsafePointer<CChar>?

// Toast overlay (issue #476)
@_silgen_name("perry_watchos_toast_active_text") func perry_watchos_toast_active_text() -> UnsafePointer<CChar>?
@_silgen_name("perry_watchos_toast_active_duration_ms") func perry_watchos_toast_active_duration_ms() -> UInt32
@_silgen_name("perry_watchos_toast_seq") func perry_watchos_toast_seq() -> UInt64
@_silgen_name("perry_watchos_toast_dismiss") func perry_watchos_toast_dismiss()

// MARK: - Observable bridge

class PerryBridge: ObservableObject {
    @Published var version: UInt64 = 0
    @Published var toastSeq: UInt64 = 0
    @Published var toastText: String? = nil
    private var timer: Timer?
    private var toastDismissWork: DispatchWorkItem?

    func start() {
        timer = Timer.scheduledTimer(withTimeInterval: 1.0 / 60.0, repeats: true) { [weak self] _ in
            guard let self = self else { return }
            // Drive JS runtime timers (setInterval, setTimeout) each frame.
            _ = js_callback_timer_tick()
            _ = js_interval_timer_tick()
            let v = perry_watchos_tree_version()
            if v != self.version {
                self.version = v
            }
            let s = perry_watchos_toast_seq()
            if s != self.toastSeq {
                self.toastSeq = s
                self.refreshActiveToast()
            }
        }
    }

    private func refreshActiveToast() {
        if let cstr = perry_watchos_toast_active_text() {
            let str = String(cString: cstr)
            self.toastText = str
            let durationMs = perry_watchos_toast_active_duration_ms()
            let interval = max(0.5, Double(durationMs) / 1000.0)
            self.toastDismissWork?.cancel()
            let work = DispatchWorkItem {
                perry_watchos_toast_dismiss()
            }
            self.toastDismissWork = work
            DispatchQueue.main.asyncAfter(deadline: .now() + interval, execute: work)
        } else {
            self.toastText = nil
            self.toastDismissWork?.cancel()
            self.toastDismissWork = nil
        }
    }
}

struct ToastBanner: View {
    let text: String
    var body: some View {
        Text(text)
            .font(.footnote)
            .foregroundColor(.white)
            .padding(.horizontal, 12)
            .padding(.vertical, 8)
            .background(
                RoundedRectangle(cornerRadius: 10)
                    .fill(Color.black.opacity(0.85))
            )
            .padding(.top, 6)
            .frame(maxWidth: .infinity, alignment: .center)
            .transition(.move(edge: .top).combined(with: .opacity))
    }
}

// MARK: - Map annotation model (issue #517)

struct PerryMapPin: Identifiable {
    let id: Int
    let coordinate: CLLocationCoordinate2D
    let title: String
}

// MARK: - Recursive SwiftUI renderer

struct NodeView: View {
    let nodeId: Int64
    @ObservedObject var bridge: PerryBridge

    var body: some View {
        if perry_watchos_node_hidden(nodeId) {
            EmptyView()
        } else {
            nodeContent
                .modifier(CommonModifiers(nodeId: nodeId))
        }
    }

    @ViewBuilder var nodeContent: some View {
        switch perry_watchos_node_kind(nodeId) {
        case 0: textView
        case 1: buttonView
        case 2: VStack(spacing: spacingValue) { children }
        case 3: HStack(spacing: spacingValue) { children }
        case 4: ZStack { children }
        case 5: Spacer()
        case 6: Divider()
        case 7: toggleView
        case 8: sliderView
        case 9: imageView
        case 10: ScrollView { children }
        case 11: progressView
        case 12: pickerView
        case 13: List { children }
        case 14: NavigationStack { children }
        case 16: mapView
        default: EmptyView()
        }
    }

    // MARK: Widget implementations

    @ViewBuilder var textView: some View {
        let t = nodeText
        let fontSize = perry_watchos_node_font_size(nodeId)
        let fontWeight = perry_watchos_node_font_weight(nodeId)

        if fontSize > 0 {
            if fontWeight >= 0 {
                Text(t).font(.system(size: fontSize, weight: swiftWeight(fontWeight)))
            } else {
                Text(t).font(.system(size: fontSize))
            }
        } else {
            Text(t)
        }
    }

    var buttonView: some View {
        Button(nodeText) {
            perry_watchos_handle_action(nodeId)
        }
    }

    var toggleView: some View {
        Toggle(nodeText, isOn: Binding(
            get: { perry_watchos_node_toggle_on(nodeId) },
            set: { perry_watchos_toggle_changed(nodeId, $0) }
        ))
    }

    var sliderView: some View {
        Slider(
            value: Binding(
                get: { perry_watchos_node_slider_value(nodeId) },
                set: { perry_watchos_slider_changed(nodeId, $0) }
            ),
            in: perry_watchos_node_slider_min(nodeId)...perry_watchos_node_slider_max(nodeId)
        )
    }

    @ViewBuilder var imageView: some View {
        let name = nodeText
        let w = perry_watchos_node_image_width(nodeId)
        let h = perry_watchos_node_image_height(nodeId)
        let img = Image(systemName: name)
            .resizable()
            .aspectRatio(contentMode: .fit)

        if w > 0 && h > 0 {
            img.frame(width: w, height: h)
        } else if w > 0 {
            img.frame(width: w)
        } else if h > 0 {
            img.frame(height: h)
        } else {
            Image(systemName: name)
        }
    }

    var progressView: some View {
        ProgressView(value: perry_watchos_node_progress_value(nodeId))
    }

    // MARK: MapView (issue #517)
    //
    // SwiftUI `Map(coordinateRegion:annotationItems:)` is available on
    // watchOS 7+. The pin overlays use the deprecated `MapMarker` API
    // because the newer `Map { Marker(...) }` shape requires watchOS 10;
    // the deprecated API still ships and runs on every shipping watch.
    // `set_map_type` is read but ignored — SwiftUI's watchOS Map doesn't
    // expose the `mapStyle` modifier on watchOS 7-9.
    @ViewBuilder var mapView: some View {
        let lat = perry_watchos_node_map_lat(nodeId)
        let lon = perry_watchos_node_map_lon(nodeId)
        let latSpan = perry_watchos_node_map_lat_span(nodeId)
        let lonSpan = perry_watchos_node_map_lon_span(nodeId)
        let region = MKCoordinateRegion(
            center: CLLocationCoordinate2D(latitude: lat, longitude: lon),
            span: MKCoordinateSpan(
                latitudeDelta: max(latSpan, 0.001),
                longitudeDelta: max(lonSpan, 0.001)
            )
        )
        let count = Int(perry_watchos_node_map_pin_count(nodeId))
        let pins: [PerryMapPin] = (0..<count).map { i in
            let plat = perry_watchos_node_map_pin_lat(nodeId, Int32(i))
            let plon = perry_watchos_node_map_pin_lon(nodeId, Int32(i))
            let title: String = {
                if let p = perry_watchos_node_map_pin_title(nodeId, Int32(i)) {
                    return String(cString: p)
                }
                return ""
            }()
            return PerryMapPin(
                id: i,
                coordinate: CLLocationCoordinate2D(latitude: plat, longitude: plon),
                title: title
            )
        }
        Map(coordinateRegion: .constant(region), annotationItems: pins) { item in
            MapMarker(coordinate: item.coordinate)
        }
    }

    var pickerView: some View {
        let count = Int(perry_watchos_node_picker_count(nodeId))
        return Picker(nodeText, selection: Binding(
            get: { Int(perry_watchos_node_picker_selected(nodeId)) },
            set: { perry_watchos_picker_changed(nodeId, Int64($0)) }
        )) {
            ForEach(0..<count, id: \.self) { i in
                if let ptr = perry_watchos_node_picker_item(nodeId, Int32(i)) {
                    Text(String(cString: ptr)).tag(i)
                }
            }
        }
    }

    // MARK: Helpers

    var nodeText: String {
        if let ptr = perry_watchos_node_text(nodeId) {
            return String(cString: ptr)
        }
        return ""
    }

    var spacingValue: CGFloat? {
        let s = perry_watchos_node_spacing(nodeId)
        return s > 0 ? s : nil
    }

    var children: some View {
        let count = perry_watchos_node_child_count(nodeId)
        return ForEach(0..<Int(count), id: \.self) { i in
            NodeView(nodeId: perry_watchos_node_child(nodeId, Int32(i)), bridge: bridge)
        }
    }

    func swiftWeight(_ w: Double) -> Font.Weight {
        switch Int(w) {
        case 1: return .ultraLight
        case 2: return .thin
        case 3: return .light
        case 4: return .regular
        case 5: return .medium
        case 6: return .semibold
        case 7: return .bold
        case 8: return .heavy
        case 9: return .black
        default: return .regular
        }
    }
}

// MARK: - Common modifiers

struct CommonModifiers: ViewModifier {
    let nodeId: Int64

    func body(content: Content) -> some View {
        var view = AnyView(content)

        // Foreground color
        if perry_watchos_node_has_color(nodeId) {
            let r = perry_watchos_node_color(nodeId, 0)
            let g = perry_watchos_node_color(nodeId, 1)
            let b = perry_watchos_node_color(nodeId, 2)
            let a = perry_watchos_node_color(nodeId, 3)
            view = AnyView(view.foregroundColor(Color(red: r, green: g, blue: b, opacity: a)))
        }

        // Background color
        if perry_watchos_node_has_bg_color(nodeId) {
            let r = perry_watchos_node_bg_color(nodeId, 0)
            let g = perry_watchos_node_bg_color(nodeId, 1)
            let b = perry_watchos_node_bg_color(nodeId, 2)
            let a = perry_watchos_node_bg_color(nodeId, 3)
            view = AnyView(view.background(Color(red: r, green: g, blue: b, opacity: a)))
        }

        // Corner radius
        let cr = perry_watchos_node_corner_radius(nodeId)
        if cr >= 0 {
            view = AnyView(view.cornerRadius(cr))
        }

        // Frame
        let fw = perry_watchos_node_frame_width(nodeId)
        let fh = perry_watchos_node_frame_height(nodeId)
        if fw >= 0 && fh >= 0 {
            view = AnyView(view.frame(width: fw, height: fh))
        } else if fw >= 0 {
            view = AnyView(view.frame(width: fw))
        } else if fh >= 0 {
            view = AnyView(view.frame(height: fh))
        }

        // Padding
        if perry_watchos_node_has_edge_insets(nodeId) {
            let top = perry_watchos_node_edge_inset(nodeId, 0)
            let left = perry_watchos_node_edge_inset(nodeId, 1)
            let bottom = perry_watchos_node_edge_inset(nodeId, 2)
            let right = perry_watchos_node_edge_inset(nodeId, 3)
            view = AnyView(view.padding(EdgeInsets(top: top, leading: left, bottom: bottom, trailing: right)))
        } else {
            let p = perry_watchos_node_padding(nodeId)
            if p >= 0 {
                view = AnyView(view.padding(p))
            }
        }

        // Opacity
        let opacity = perry_watchos_node_opacity(nodeId)
        if opacity < 1.0 {
            view = AnyView(view.opacity(opacity))
        }

        // Disabled
        if !perry_watchos_node_enabled(nodeId) {
            view = AnyView(view.disabled(true))
        }

        return view
    }
}

// MARK: - App entry point

// perry/background (#538) — receive WKRefreshBackgroundTask deliveries.
// Reads our `userInfo["perry_id"]` and forwards to Rust; calls
// `setTaskCompletedWithSnapshot(false)` on each task to release the OS budget.
final class PerryWatchAppDelegate: NSObject, WKApplicationDelegate {
    func handle(_ backgroundTasks: Set<WKRefreshBackgroundTask>) {
        for task in backgroundTasks {
            if let refresh = task as? WKApplicationRefreshBackgroundTask {
                if let info = refresh.userInfo as? [String: Any],
                   let id = info["perry_id"] as? String {
                    id.withCString { cstr in
                        perry_watchos_dispatch_background_task(cstr)
                    }
                }
                refresh.setTaskCompletedWithSnapshot(false)
            } else {
                // Other refresh kinds (URLSession, snapshot, etc.) are not
                // currently routed through perry/background.
                task.setTaskCompletedWithSnapshot(false)
            }
        }
    }
}

@main
struct PerryApp: App {
    @WKApplicationDelegateAdaptor(PerryWatchAppDelegate.self) var appDelegate
    @StateObject private var bridge = PerryBridge()

    init() {
        perry_main_init()
    }

    var body: some Scene {
        WindowGroup {
            ZStack(alignment: .top) {
                let rootId = perry_watchos_root_node()
                if rootId > 0 {
                    NodeView(nodeId: rootId, bridge: bridge)
                        .onAppear { bridge.start() }
                } else {
                    Text("Perry watchOS App")
                        .onAppear { bridge.start() }
                }
                if let msg = bridge.toastText {
                    ToastBanner(text: msg)
                        .animation(.easeInOut(duration: 0.2), value: bridge.toastSeq)
                }
            }
        }
    }
}
