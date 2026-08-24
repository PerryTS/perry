// Type declarations for iOS-specific Perry APIs.

/** UIKit size class for the active scene. */
export type LayoutSizeClass = "compact" | "regular" | "unspecified";

/** How the active scene currently occupies its display. */
export type WindowMode = "fullScreen" | "sideBySide" | "windowed";

/**
 * A scene-relative layout snapshot. Values are expressed in UIKit points,
 * not physical pixels. Use these values instead of device-model checks: the
 * same iPad can move between full screen, Split View, and Stage Manager at
 * runtime, and future display shapes can expose different safe areas.
 */
export interface LayoutEnvironment {
  width: number;
  height: number;
  aspectRatio: number;
  displayScale: number;
  horizontalSizeClass: LayoutSizeClass;
  verticalSizeClass: LayoutSizeClass;
  orientation: "portrait" | "landscape" | "square";
  windowMode: WindowMode;
  isMultitasking: boolean;
  isFourByThree: boolean;
  /** iOS 27 effective scene frame in the system display coordinate space. */
  systemFrameX: number;
  systemFrameY: number;
  systemFrameWidth: number;
  systemFrameHeight: number;
  /** Whether iOS 27 is currently delivering an interactive window resize. */
  isInteractivelyResizing: boolean;
  /** Whether the scene's interface orientation is currently locked. */
  isInterfaceOrientationLocked: boolean;
  safeAreaTop: number;
  safeAreaRight: number;
  safeAreaBottom: number;
  safeAreaLeft: number;
}

/** Return the current active UIWindowScene's adaptive-layout environment. */
export function getLayoutEnvironment(): LayoutEnvironment;

/**
 * Subscribe to scene geometry, size-class, and safe-area changes. The handler
 * receives an initial snapshot when a scene is available and then only when
 * the snapshot changes. Returns a 1-based subscription handle.
 */
export function onLayoutChange(
  callback: (environment: LayoutEnvironment) => void,
): number;

/** Remove a layout subscription. Unknown handles are ignored. */
export function offLayoutChange(subscription: number): void;

/** Availability of Apple's default system language model. */
export type FoundationModelAvailability =
  | "available"
  | "deviceNotEligible"
  | "appleIntelligenceNotEnabled"
  | "modelNotReady"
  | "unsupported";

/** Opaque, process-local Foundation Models session handle. */
export type LanguageModelSession = number & {
  readonly __perryLanguageModelSession: unique symbol;
};

/** Query the default model before creating a session. */
export function foundationModelAvailability(): FoundationModelAvailability;

/**
 * Create a conversational Foundation Models session. Reusing the handle keeps
 * the session transcript/context between `respond` calls. An empty instruction
 * string creates a session without system instructions. Returns `0` when the
 * framework is unavailable on this OS.
 */
export function createLanguageModelSession(
  instructions?: string,
): LanguageModelSession;

/** Generate an unstructured string response for a prompt. */
export function respond(
  session: LanguageModelSession,
  prompt: string,
): Promise<string>;

/** Destroy a session. Pending responses are allowed to finish. */
export function destroyLanguageModelSession(
  session: LanguageModelSession,
): void;
