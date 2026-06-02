// Native iOS QR scanner — bridges
// `dioxus_wallet::qr_scanner_ios::IosQrScanner` to an
// AVCaptureSession-backed UIViewController. Mirrors the Android
// adapter (`qr_scanner_android.rs` ↔ `QrScanBridge.kt`).
//
// ## Wire protocol with Rust
//
// Rust calls `iosqr_present_scanner(token)` (declared `extern "C"`
// in `qr_scanner_ios.rs`). We post the scanner present onto the
// main thread + return 1 if dispatch succeeded, 0 if no key
// window was reachable.
//
// When the scanner resolves — barcode detected, user cancels, or
// permission denied — we call back into Rust via
// `iosqr_deliver_result(token, outcome, payload)`:
//
//   outcome = 1 (OK)          → payload = decoded barcode text
//   outcome = 2 (CANCELLED)   → payload may be NULL
//   outcome = 3 (UNAVAILABLE) → payload = reason string
//
// Rust uses these to surface `Ok(String)` / `QrScanError::Cancelled`
// / `QrScanError::Unavailable(reason)`.
//
// ## Permission story
//
// `NSCameraUsageDescription` in `Info.plist` triggers the system's
// permission alert on the first `AVCaptureSession.startRunning()`
// call. If the user denies, AVCaptureSession's input add silently
// fails — we detect via `device.isAccessAuthorized` (synchronous
// check) and report `OUTCOME_UNAVAILABLE` immediately.
//
// ## Simulator caveat
//
// iOS Simulator returns a synthetic camera feed without decodable
// barcodes — the scanner opens but never resolves. Real-device
// testing is required for end-to-end validation. The paste-fallback
// under Diagnostics → Bootstrap covers the simulator workflow.

import AVFoundation
import UIKit

// Rust-side callbacks. Both are exported from the Rust cdylib
// via `#[unsafe(no_mangle)] pub extern "C"`; `@_silgen_name` tells
// the Swift compiler to look up the matching C-ABI symbol at
// link time without going through Swift's name mangling.
@_silgen_name("iosqr_deliver_result")
private func iosqrDeliverResult(
    _ token: UInt64,
    _ outcome: UInt8,
    _ payload: UnsafePointer<CChar>?
)

/// Hand Rust a pointer to `iosqrPresentScanner` at app startup.
/// Returns 1 on first registration, 0 if already registered
/// (idempotent — `OnceLock` semantics on the Rust side).
@_silgen_name("iosqr_register_present_fn")
@discardableResult
private func iosqrRegisterPresentFn(
    _ present: @convention(c) (UInt64) -> Int32
) -> Int32

private let OUTCOME_OK: UInt8 = 1
private let OUTCOME_CANCELLED: UInt8 = 2
private let OUTCOME_UNAVAILABLE: UInt8 = 3

/// Call this once from `App.init` (or any other early app
/// boot site) so the Rust side's `IosQrScanner::scan` can fire
/// the scanner. Idempotent.
public func iosqrInstall() {
    iosqrRegisterPresentFn(iosqrPresentScanner)
}

/// Implementation of the function Rust calls via the
/// `iosqr_register_present_fn`-stashed pointer. Presents the
/// scanner over the key window's root view controller; returns
/// 1 on dispatch success. `@convention(c)` would make it a C
/// function pointer; Swift implicitly provides that conversion
/// when we pass `iosqrPresentScanner` into
/// `iosqrRegisterPresentFn` (parameter type is annotated
/// `@convention(c)` on the Rust-imported declaration).
private func iosqrPresentScanner(_ token: UInt64) -> Int32 {
    // Find the key window's root view controller. SwiftUI's
    // WindowGroup attaches the root via UIApplication.shared
    // .connectedScenes — pick the foreground active one.
    guard let scene = UIApplication.shared.connectedScenes
        .first(where: { $0.activationState == .foregroundActive })
        as? UIWindowScene,
        let root = scene.windows.first(where: { $0.isKeyWindow })?.rootViewController
    else {
        return 0
    }
    DispatchQueue.main.async {
        presentScanner(token: token, presenter: root)
    }
    return 1
}

private func presentScanner(token: UInt64, presenter: UIViewController) {
    // Cheap synchronous permission check. Returns immediately if
    // the user has already granted or denied; `notDetermined` lets
    // AVCaptureSession.startRunning trigger the permission alert.
    let status = AVCaptureDevice.authorizationStatus(for: .video)
    switch status {
    case .denied, .restricted:
        deliverUnavailable(
            token: token,
            reason: "camera permission denied or restricted"
        )
        return
    default:
        break
    }

    let vc = QrScannerViewController(token: token)
    vc.modalPresentationStyle = .fullScreen
    // Pick the deepest currently-presented controller as the
    // presenter so we don't bury the scanner under the Dioxus
    // WebView's own modals.
    var top = presenter
    while let next = top.presentedViewController {
        top = next
    }
    top.present(vc, animated: true)
}

private func deliver(token: UInt64, outcome: UInt8, payload: String?) {
    if let s = payload {
        s.withCString { cstr in
            iosqrDeliverResult(token, outcome, cstr)
        }
    } else {
        iosqrDeliverResult(token, outcome, nil)
    }
}

private func deliverUnavailable(token: UInt64, reason: String) {
    deliver(token: token, outcome: OUTCOME_UNAVAILABLE, payload: reason)
}

/// AVCaptureSession-backed scanner. Single-shot: the first
/// successful barcode delivery dismisses the view + reports back.
final class QrScannerViewController: UIViewController,
    AVCaptureMetadataOutputObjectsDelegate {

    private let token: UInt64
    private let session = AVCaptureSession()
    private var previewLayer: AVCaptureVideoPreviewLayer?
    /// Set to true once we've delivered a result so the metadata
    /// delegate callback doesn't fire twice if AVFoundation hands
    /// us multiple frames before the dismiss settles.
    private var delivered = false

    init(token: UInt64) {
        self.token = token
        super.init(nibName: nil, bundle: nil)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("not designed for storyboard instantiation")
    }

    override func viewDidLoad() {
        super.viewDidLoad()
        view.backgroundColor = .black

        // Set up the capture input. On simulator the synthetic
        // camera is present but doesn't decode barcodes; on real
        // devices the rear camera is the default.
        guard let device = AVCaptureDevice.default(for: .video),
            let input = try? AVCaptureDeviceInput(device: device),
            session.canAddInput(input)
        else {
            // Either no device at all (rare) or device exists but
            // can't be opened — fall back to UNAVAILABLE.
            deliverOnce(
                outcome: OUTCOME_UNAVAILABLE,
                payload: "no video capture device available"
            )
            return
        }
        session.addInput(input)

        // Metadata output for QR codes specifically. Filtering at
        // the AVFoundation layer avoids waking the delegate for
        // codes we don't care about (EAN, ITF, etc.).
        let output = AVCaptureMetadataOutput()
        if session.canAddOutput(output) {
            session.addOutput(output)
            output.setMetadataObjectsDelegate(self, queue: .main)
            // .qr is the QR-code metadata type. The full set
            // includes aztec, pdf417, etc.; the wallet only cares
            // about QR for now.
            if output.availableMetadataObjectTypes.contains(.qr) {
                output.metadataObjectTypes = [.qr]
            } else {
                deliverOnce(
                    outcome: OUTCOME_UNAVAILABLE,
                    payload: "QR metadata type unavailable on this device"
                )
                return
            }
        } else {
            deliverOnce(
                outcome: OUTCOME_UNAVAILABLE,
                payload: "could not attach metadata output"
            )
            return
        }

        let preview = AVCaptureVideoPreviewLayer(session: session)
        preview.videoGravity = .resizeAspectFill
        preview.frame = view.layer.bounds
        view.layer.addSublayer(preview)
        self.previewLayer = preview

        // A simple "Cancel" button in the top-left so the user has
        // a way out if no barcode is visible.
        let cancel = UIButton(type: .system)
        cancel.setTitle("Cancel", for: .normal)
        cancel.setTitleColor(.white, for: .normal)
        cancel.titleLabel?.font = .systemFont(ofSize: 18, weight: .semibold)
        cancel.translatesAutoresizingMaskIntoConstraints = false
        cancel.addTarget(self, action: #selector(onCancel), for: .touchUpInside)
        view.addSubview(cancel)
        NSLayoutConstraint.activate([
            cancel.leadingAnchor.constraint(
                equalTo: view.safeAreaLayoutGuide.leadingAnchor, constant: 16),
            cancel.topAnchor.constraint(
                equalTo: view.safeAreaLayoutGuide.topAnchor, constant: 12),
        ])

        // AVCaptureSession.startRunning is blocking; AVFoundation
        // recommends moving it off the main thread to avoid a UI
        // hitch on session boot.
        DispatchQueue.global(qos: .userInitiated).async { [weak self] in
            self?.session.startRunning()
        }
    }

    override func viewDidLayoutSubviews() {
        super.viewDidLayoutSubviews()
        previewLayer?.frame = view.layer.bounds
    }

    @objc private func onCancel() {
        deliverOnce(outcome: OUTCOME_CANCELLED, payload: nil)
    }

    func metadataOutput(
        _ output: AVCaptureMetadataOutput,
        didOutput objects: [AVMetadataObject],
        from _: AVCaptureConnection
    ) {
        guard !delivered,
            let qr = objects.first as? AVMetadataMachineReadableCodeObject,
            qr.type == .qr,
            let text = qr.stringValue
        else {
            return
        }
        deliverOnce(outcome: OUTCOME_OK, payload: text)
    }

    /// Dismiss the scanner + deliver the result through the Rust
    /// callback. Idempotent — `delivered` guards against repeat
    /// fires from a noisy metadata stream.
    private func deliverOnce(outcome: UInt8, payload: String?) {
        guard !delivered else { return }
        delivered = true
        // Stop the session before dismissing so the camera LED
        // turns off promptly. Off the main thread because
        // stopRunning is also blocking.
        DispatchQueue.global(qos: .userInitiated).async { [weak self] in
            self?.session.stopRunning()
        }
        DispatchQueue.main.async { [weak self] in
            guard let self = self else { return }
            self.dismiss(animated: true) {
                deliver(token: self.token, outcome: outcome, payload: payload)
            }
        }
    }
}
