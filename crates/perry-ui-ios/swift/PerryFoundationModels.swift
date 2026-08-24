import Foundation
@_weakLinked import FoundationModels

public typealias PerryFoundationModelCompletion = @convention(c) (
    Int64,
    Bool,
    UnsafePointer<UInt8>?,
    Int32
) -> Void

private func decodeUTF8(_ bytes: UnsafePointer<UInt8>?, _ length: Int32) -> String {
    guard let bytes, length > 0 else { return "" }
    return String(decoding: UnsafeBufferPointer(start: bytes, count: Int(length)), as: UTF8.self)
}

private func complete(
    _ callback: PerryFoundationModelCompletion,
    context: Int64,
    success: Bool,
    value: String
) {
    let bytes = Array(value.utf8)
    bytes.withUnsafeBufferPointer { buffer in
        callback(context, success, buffer.baseAddress, Int32(buffer.count))
    }
}

@available(iOS 26.0, *)
private final class PerryLanguageModelSessions: @unchecked Sendable {
    static let shared = PerryLanguageModelSessions()

    private let lock = NSLock()
    private var nextHandle: Int64 = 1
    private var sessions: [Int64: LanguageModelSession] = [:]

    func create(instructions: String) -> Int64 {
        guard SystemLanguageModel.default.isAvailable else { return 0 }
        let session = LanguageModelSession(
            instructions: instructions.isEmpty ? nil : instructions
        )
        lock.lock()
        defer { lock.unlock() }
        let handle = nextHandle
        nextHandle += 1
        sessions[handle] = session
        return handle
    }

    func session(for handle: Int64) -> LanguageModelSession? {
        lock.lock()
        defer { lock.unlock() }
        return sessions[handle]
    }

    func destroy(_ handle: Int64) {
        lock.lock()
        sessions.removeValue(forKey: handle)
        lock.unlock()
    }
}

@_cdecl("perry_swift_foundation_model_availability")
public func perrySwiftFoundationModelAvailability() -> Int32 {
    guard #available(iOS 26.0, *) else { return 0 }
    switch SystemLanguageModel.default.availability {
    case .available:
        return 1
    case .unavailable(.deviceNotEligible):
        return 2
    case .unavailable(.appleIntelligenceNotEnabled):
        return 3
    case .unavailable(.modelNotReady):
        return 4
    @unknown default:
        return 0
    }
}

@_cdecl("perry_swift_foundation_model_session_create")
public func perrySwiftFoundationModelSessionCreate(
    _ bytes: UnsafePointer<UInt8>?,
    _ length: Int32
) -> Int64 {
    guard #available(iOS 26.0, *) else { return 0 }
    return PerryLanguageModelSessions.shared.create(instructions: decodeUTF8(bytes, length))
}

@_cdecl("perry_swift_foundation_model_session_destroy")
public func perrySwiftFoundationModelSessionDestroy(_ session: Int64) {
    guard #available(iOS 26.0, *) else { return }
    PerryLanguageModelSessions.shared.destroy(session)
}

@_cdecl("perry_swift_foundation_model_respond")
public func perrySwiftFoundationModelRespond(
    _ sessionHandle: Int64,
    _ bytes: UnsafePointer<UInt8>?,
    _ length: Int32,
    _ context: Int64,
    _ callback: PerryFoundationModelCompletion
) {
    guard #available(iOS 26.0, *) else {
        complete(callback, context: context, success: false, value: "Foundation Models requires iOS 26 or later")
        return
    }
    guard let session = PerryLanguageModelSessions.shared.session(for: sessionHandle) else {
        complete(callback, context: context, success: false, value: "Invalid or unavailable Foundation Models session")
        return
    }
    let prompt = decodeUTF8(bytes, length)
    Task {
        do {
            let response = try await session.respond(to: prompt)
            complete(callback, context: context, success: true, value: response.content)
        } catch {
            complete(callback, context: context, success: false, value: String(describing: error))
        }
    }
}
