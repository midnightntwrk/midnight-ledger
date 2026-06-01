package io.iohk.midnight.wallet

import android.app.Activity
import android.util.Log
import com.google.android.gms.common.ConnectionResult
import com.google.android.gms.common.GoogleApiAvailability
import com.google.mlkit.common.MlKitException
import com.google.mlkit.vision.barcode.common.Barcode
import com.google.mlkit.vision.codescanner.GmsBarcodeScannerOptions
import com.google.mlkit.vision.codescanner.GmsBarcodeScanning

/**
 * Thin Kotlin shell wrapping Google ML Kit's Code Scanner. Called
 * from Rust via JNI; each in-flight scan carries a `token: Long`
 * the Rust side uses to find the waiting oneshot::Sender.
 *
 * Why ML Kit: it owns the camera, viewfinder UI, focus indicator,
 * torch toggle, and the runtime permission prompt. Anything we'd
 * have to wire by hand with CameraX + a custom Activity comes for
 * free here. The only cost is a hard dep on Google Play Services
 * — devices without it (GrapheneOS, Huawei post-2020, AOSP-only
 * emulators) surface as `Play Services unavailable` and the wallet
 * falls back to the paste field in Diagnostics → Bootstrap.
 *
 * Restricted to FORMAT_QR_CODE because the wallet only consumes
 * `openid4vp://` / `openid-credential-offer://` URLs encoded as
 * QR codes. Letting ML Kit hand back Code-128 / EAN / PDF417
 * would be misleading.
 *
 * Both success and failure paths route through the same JNI
 * callback (`nativeOnQrResult`). On the Rust side, a non-null
 * `error` distinguishes the failure path; the string
 * `"cancelled"` maps to `QrScanError::Cancelled`, anything else
 * to `QrScanError::Decoder` (with `"Play Services unavailable"`
 * recognised specifically as `QrScanError::Unavailable`).
 */
class QrScanBridge {
    companion object {
        private const val TAG = "QrScanBridge"

        /**
         * Implemented in libdioxuswalletmain.so via
         * `Java_io_iohk_midnight_wallet_QrScanBridge_nativeOnQrResult`.
         * `url` is non-null on success; `error` is non-null on
         * failure (or cancellation — ML Kit reports both via
         * `MlKitException.CODE_SCANNER_CANCELLED`, so the Rust
         * side maps the string `"cancelled"` to
         * `QrScanError::Cancelled`).
         */
        @JvmStatic
        external fun nativeOnQrResult(token: Long, url: String?, error: String?)

        /**
         * Entry point called from Rust. Verifies Play Services is
         * present, builds the QR-only client, and starts the scan.
         * Result flows back through `nativeOnQrResult` on either
         * path.
         *
         * Safe to call from any thread — ML Kit's `startScan()`
         * dispatches the camera launch to the main UI thread
         * internally, and the listeners always fire on the main
         * thread.
         */
        @JvmStatic
        fun startScan(activity: Activity, token: Long) {
            try {
                val availability = GoogleApiAvailability.getInstance()
                val resultCode = availability.isGooglePlayServicesAvailable(activity)
                if (resultCode != ConnectionResult.SUCCESS) {
                    val desc =
                        availability.getErrorString(resultCode) ?: "code=$resultCode"
                    Log.w(TAG, "Play Services unavailable: $desc")
                    nativeOnQrResult(
                        token,
                        null,
                        "Play Services unavailable: $desc",
                    )
                    return
                }

                val options = GmsBarcodeScannerOptions.Builder()
                    .setBarcodeFormats(Barcode.FORMAT_QR_CODE)
                    .build()
                val scanner = GmsBarcodeScanning.getClient(activity, options)

                scanner.startScan()
                    .addOnSuccessListener { barcode ->
                        val raw = barcode.rawValue
                        if (raw.isNullOrEmpty()) {
                            Log.w(TAG, "empty QR payload (token=$token)")
                            nativeOnQrResult(token, null, "empty QR payload")
                        } else {
                            Log.i(TAG, "scan ok (token=$token, len=${raw.length})")
                            nativeOnQrResult(token, raw, null)
                        }
                    }
                    .addOnFailureListener { e ->
                        val msg = when {
                            e is MlKitException &&
                                e.errorCode == MlKitException.CODE_SCANNER_CANCELLED ->
                                "cancelled"
                            else -> e.message ?: "scan failed"
                        }
                        Log.w(TAG, "scan failed (token=$token): $msg", e)
                        nativeOnQrResult(token, null, msg)
                    }
            } catch (e: Throwable) {
                // Defensive — any sync exception during
                // configuration (e.g. ML Kit not on classpath at
                // runtime) must surface as a Decoder error rather
                // than crash the process.
                Log.e(TAG, "startScan threw (token=$token)", e)
                nativeOnQrResult(token, null, e.message ?: "startScan threw")
            }
        }
    }
}
