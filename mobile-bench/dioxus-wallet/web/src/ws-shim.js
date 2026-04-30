// Shim for `import { WebSocket } from "ws"`. midnight-did-api does a
// named import; isomorphic-ws only exposes a default. Re-export the
// browser-native WebSocket as a named export so the import shape
// upstream uses keeps working.
export const WebSocket = globalThis.WebSocket;
export default globalThis.WebSocket;
