// Re-export the `buffer` polyfill globally so deps that do
// `Buffer.from(...)` without an explicit import keep working in the
// WebView.
import { Buffer } from "buffer";
globalThis.Buffer = Buffer;
export { Buffer };
