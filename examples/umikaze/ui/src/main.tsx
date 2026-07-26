import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import App from "./App";

// Signed PAKs need only the publisher's public Ed25519 key in a WebView. The
// private signing material stays in CI; this hook is intentionally a small
// host boundary so the same presentation can run in a browser or Tauri.
const verificationKeyId = import.meta.env.VITE_ARIA_PAK_VERIFICATION_KEY_ID;
const verificationKeyHex = import.meta.env.VITE_ARIA_PAK_VERIFICATION_KEY_HEX;
if (verificationKeyId && verificationKeyHex) {
  globalThis.ariaPakKeyProvider = async () => ({
    verification_key_id: verificationKeyId,
    verification_key_hex: verificationKeyHex,
    encryption_key_id: "",
    encryption_key_hex: "",
  });
}

if ("serviceWorker" in navigator && window.location.protocol.startsWith("http")) {
  void navigator.serviceWorker.register(new URL("./service-worker.js", document.baseURI));
}

createRoot(document.getElementById("root")!).render(
  <StrictMode><App /></StrictMode>,
);
