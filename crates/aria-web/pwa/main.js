import { WebAudioAdapter } from "./web-audio.js";
import { IndexedDbSaveStore } from "./save-store.js";
import { createWebRenderer } from "./web-renderer.js";

const canvas = document.querySelector("#aria-canvas");
const status = document.querySelector("#aria-status");
const updateButton = document.querySelector("#aria-update");
const pressed = new Set();
const held = new Set();
const heldSources = new Map();
const activeGamepadSources = new Map();
const pointer = { x: 0, y: 0, primary_pressed: false, primary_held: false };
let pointerPresent = false;
let scrollDeltaY = 0;
let sequence = 0;
let logicalSize = { width: 1280, height: 720 };

const keyActions = new Map([
  ["ArrowUp", "navigate_up"],
  ["ArrowDown", "navigate_down"],
  ["ArrowLeft", "navigate_left"],
  ["ArrowRight", "navigate_right"],
  ["Enter", "confirm"],
  [" ", "advance"],
  ["Escape", "cancel"],
  ["ContextMenu", "menu"],
  ["Control", "skip"],
]);

function setStatus(message) {
  status.textContent = message;
  status.hidden = !message;
}

function pressSource(action, source) {
  let sources = heldSources.get(action);
  if (!sources) {
    sources = new Set();
    heldSources.set(action, sources);
  }
  if (sources.size === 0) pressed.add(action);
  sources.add(source);
  held.add(action);
}

function releaseSource(action, source) {
  const sources = heldSources.get(action);
  if (!sources) return;
  sources.delete(source);
  if (sources.size === 0) {
    heldSources.delete(action);
    held.delete(action);
  }
}

function syncGamepads() {
  if (typeof navigator.getGamepads !== "function") return;
  const next = new Map();
  let pads;
  try {
    pads = navigator.getGamepads() || [];
  } catch {
    return;
  }
  for (const pad of pads) {
    if (!pad) continue;
    const addButton = (index, action) => {
      const button = pad.buttons[index];
      const source = `gamepad:${pad.index}:button:${index}`;
      const wasHeld = activeGamepadSources.get(source) === action;
      if (
        button?.pressed ||
        (button?.value || 0) >= 0.5 ||
        (wasHeld && (button?.value || 0) >= 0.35)
      ) {
        next.set(source, action);
      }
    };
    addButton(0, "confirm");
    addButton(1, "cancel");
    addButton(7, "skip");
    addButton(9, "menu");
    addButton(12, "navigate_up");
    addButton(13, "navigate_down");
    addButton(14, "navigate_left");
    addButton(15, "navigate_right");
    const horizontal = pad.axes[0] || 0;
    const vertical = pad.axes[1] || 0;
    const addAxis = (source, action, value, sign) => {
      const wasHeld = activeGamepadSources.get(source) === action;
      const pressed = sign < 0 ? value <= -0.5 : value >= 0.5;
      const retained = wasHeld && (sign < 0 ? value <= -0.35 : value >= 0.35);
      if (pressed || retained) next.set(source, action);
    };
    addAxis(`gamepad:${pad.index}:axis:left`, "navigate_left", horizontal, -1);
    addAxis(`gamepad:${pad.index}:axis:right`, "navigate_right", horizontal, 1);
    addAxis(`gamepad:${pad.index}:axis:up`, "navigate_up", vertical, -1);
    addAxis(`gamepad:${pad.index}:axis:down`, "navigate_down", vertical, 1);
  }
  for (const [source, action] of activeGamepadSources) {
    if (!next.has(source)) releaseSource(action, source);
  }
  for (const [source, action] of next) {
    if (!activeGamepadSources.has(source)) pressSource(action, source);
  }
  activeGamepadSources.clear();
  for (const [source, action] of next) activeGamepadSources.set(source, action);
}

function resizeCanvas() {
  const ratio = Math.max(1, window.devicePixelRatio || 1);
  const bounds = canvas.getBoundingClientRect();
  canvas.width = Math.max(1, Math.round(bounds.width * ratio));
  canvas.height = Math.max(1, Math.round(bounds.height * ratio));
}

// This value is part of every Core input rather than a renderer-only resize
// detail.  It makes the responsive branch selected by a PWA replay exactly
// reproducible by the Native player (and vice versa). `canvas` already lives
// inside the CSS safe-area padding, so the remaining insets are zero in its
// local coordinate system.
function uiViewport() {
  const ratio = Math.max(1, window.devicePixelRatio || 1);
  const bounds = canvas.getBoundingClientRect();
  return {
    width: Math.max(1, Math.round(bounds.width * ratio)),
    height: Math.max(1, Math.round(bounds.height * ratio)),
    scale_factor: ratio,
    safe_area: { top: 0, right: 0, bottom: 0, left: 0 },
  };
}

function pointerPosition(event) {
  const bounds = canvas.getBoundingClientRect();
  const scale = Math.min(bounds.width / logicalSize.width, bounds.height / logicalSize.height);
  const contentWidth = logicalSize.width * scale;
  const contentHeight = logicalSize.height * scale;
  const offsetX = (bounds.width - contentWidth) * 0.5;
  const offsetY = (bounds.height - contentHeight) * 0.5;
  pointer.x = Math.min(
    logicalSize.width,
    Math.max(0, (event.clientX - bounds.left - offsetX) / scale),
  );
  pointer.y = Math.min(
    logicalSize.height,
    Math.max(0, (event.clientY - bounds.top - offsetY) / scale),
  );
  pointerPresent = true;
}

window.addEventListener("resize", resizeCanvas);
window.addEventListener("keydown", (event) => {
  const action = keyActions.get(event.key);
  if (!action) return;
  event.preventDefault();
  pressSource(action, `key:${event.code || event.key}`);
});
window.addEventListener("keyup", (event) => {
  const action = keyActions.get(event.key);
  if (action) releaseSource(action, `key:${event.code || event.key}`);
});
window.addEventListener("blur", () => {
  for (const [action, sources] of heldSources) {
    for (const source of sources) releaseSource(action, source);
  }
  activeGamepadSources.clear();
  pointer.primary_held = false;
});
canvas.addEventListener("pointermove", pointerPosition);
canvas.addEventListener("pointerdown", (event) => {
  pointerPosition(event);
  pointer.primary_pressed = true;
  pointer.primary_held = true;
  canvas.focus({ preventScroll: true });
  canvas.setPointerCapture(event.pointerId);
});
canvas.addEventListener("pointerup", (event) => {
  pointerPosition(event);
  pointer.primary_held = false;
});
canvas.addEventListener("wheel", (event) => {
  event.preventDefault();
  scrollDeltaY += event.deltaY;
}, { passive: false });

async function registerServiceWorker() {
  if (!("serviceWorker" in navigator)) return;
  // The first worker claims this page after its cache is ready. Reloading at
  // that exact moment aborts the initial PAK streaming request, producing a
  // spurious startup failure before the second load succeeds. Only a page
  // that already had a controller should reload for a newly activated update.
  const hadController = Boolean(navigator.serviceWorker.controller);
  const registration = await navigator.serviceWorker.register("./service-worker.js", {
    scope: "./",
  });
  registration.addEventListener("updatefound", () => {
    const worker = registration.installing;
    worker?.addEventListener("statechange", () => {
      if (worker.state === "installed" && navigator.serviceWorker.controller) {
        updateButton.hidden = false;
      }
    });
  });
  updateButton.addEventListener("click", () => {
    registration.waiting?.postMessage({ type: "SKIP_WAITING" });
  });
  navigator.serviceWorker.addEventListener("controllerchange", () => {
    if (hadController) location.reload();
  });
}

async function loadBundledFonts(fontAssets, readAsset) {
  if (!Array.isArray(fontAssets)) {
    throw new Error("bundle.aria.json font_assets must be an array");
  }
  if (fontAssets.length === 0) return [];
  if (typeof FontFace !== "function" || !document.fonts) {
    throw new Error("this browser cannot load the bundled font contract");
  }
  const families = [];
  for (const [index, logicalPath] of fontAssets.entries()) {
    if (typeof logicalPath !== "string" || !logicalPath) {
      throw new Error("bundle.aria.json contains an invalid font asset path");
    }
    const bytes = readAsset(logicalPath);
    if (!(bytes instanceof Uint8Array) || bytes.byteLength === 0) {
      throw new Error(`pak font asset '${logicalPath}' was not returned as non-empty Uint8Array`);
    }
    // Generated names prevent an internal font family name from being
    // interpreted differently by browser/OS font registries. The bytes are
    // copied out of WASM memory before FontFace retains them.
    const family = `AriaEngineBundledFont${index}`;
    const source = bytes.buffer.slice(bytes.byteOffset, bytes.byteOffset + bytes.byteLength);
    const face = new FontFace(family, source, { display: "block" });
    await face.load();
    document.fonts.add(face);
    families.push(family);
  }
  await document.fonts.ready;
  return families;
}

async function boot() {
  await registerServiceWorker();
  resizeCanvas();
  const { default: init, WebPak, WebRuntime } = await import("./pkg/aria_web.js");
  await init();
  const [bundleResponse, bytecodeResponse] = await Promise.all([
    fetch("./bundle.aria.json"),
    fetch("./game.ariac"),
  ]);
  if (!bundleResponse.ok) {
    throw new Error(`bundle.aria.json: HTTP ${bundleResponse.status}`);
  }
  if (!bytecodeResponse.ok) throw new Error(`game.ariac: HTTP ${bytecodeResponse.status}`);
  const bundle = await bundleResponse.json();
  if (bundle.schema_version !== 5 || ![2, 4].includes(bundle.vm_abi_version)) {
    throw new Error("unsupported Aria portable bundle");
  }
  logicalSize = { width: bundle.logical_width, height: bundle.logical_height };
  const bytecode = new Uint8Array(await bytecodeResponse.arrayBuffer());
  const runtime = new WebRuntime(
    bytecode,
    bundle.logical_width,
    bundle.logical_height,
  );
  const packs = bundle.pak_packs?.length
    ? bundle.pak_packs
    : [{
      pack_id: bundle.pack_id,
      file: "game.ariapak",
      size: bundle.pak_size,
      content_root_blake3: bundle.pak_content_root_blake3,
      assets: bundle.font_assets,
    }];
  const mounted = [];
  for (const declaration of packs) {
    const response = await fetch(`./${declaration.file}`);
    if (!response.ok) throw new Error(`${declaration.file}: HTTP ${response.status}`);
    const bytes = new Uint8Array(await response.arrayBuffer());
    if (bytes.byteLength !== declaration.size) {
      throw new Error(`${declaration.file}: size does not match bundle.aria.json`);
    }
    let pak;
    if (bundle.pak_profile === "dev") {
      pak = new WebPak(bytes);
    } else {
      // Signed and protected Web builds receive their trust material from the
      // host integration. The hook is deliberately outside the VM and only
      // returns the values needed by the adapter-owned package reader.
      const keyProvider = globalThis.ariaPakKeyProvider;
      if (typeof keyProvider !== "function") {
        throw new Error("signed/protected Aria package requires ariaPakKeyProvider(bundle)");
      }
      const keys = await keyProvider(bundle);
      pak = WebPak.new_with_keys(
        bytes,
        keys.verification_key_id,
        keys.verification_key_hex,
        keys.encryption_key_id || "",
        keys.encryption_key_hex || "",
      );
    }
    if (pak.game_id() !== bundle.game_id
      || pak.content_root_blake3() !== declaration.content_root_blake3) {
      throw new Error(`${declaration.file}: metadata does not match bundle.aria.json`);
    }
    mounted.push(pak);
  }
  const readAsset = (logicalPath) => {
    let lastError;
    for (const pak of mounted) {
      try {
        return pak.read(logicalPath);
      } catch (error) {
        lastError = error;
      }
    }
    throw lastError || new Error(`missing asset '${logicalPath}'`);
  };
  globalThis.ariaAssetBytes = readAsset;
  const fontFamilies = await loadBundledFonts(bundle.font_assets, readAsset);
  const renderer = await createWebRenderer(canvas, readAsset, {
    onStatus: setStatus,
    fontFamilies,
  });
  globalThis.__ariaRendererBackend = renderer.backend;
  const audio = new WebAudioAdapter(readAsset);
  audio.installUnlock(document);
  const saves = new IndexedDbSaveStore(`aria-v3-${bundle.save_namespace}`, 3);
  await saves.open();
  // Renderer readiness is not a game UI element. Leaving a fixed technical
  // pill over a title screen made the PWA look unfinished and hid authored
  // controls; status is now reserved for recovery/errors.
  setStatus("");

  let previous = performance.now();
  async function frame(now) {
    const delta = Math.min(250, Math.max(0, Math.round(now - previous)));
    previous = now;
    syncGamepads();
    const input = {
      sequence: ++sequence,
      delta_ms: delta,
      pressed: [...pressed],
      held: [...held],
      pointer: pointerPresent ? { ...pointer } : null,
      scroll_delta_y: scrollDeltaY,
      viewport: uiViewport(),
    };
    pressed.clear();
    pointer.primary_pressed = false;
    scrollDeltaY = 0;
    const output = JSON.parse(runtime.step(JSON.stringify(input)));
    logicalSize = output.scene.logical_size || logicalSize;
    await audio.consume(output.audio);
    await renderer.submit(output.scene);
    globalThis.__ariaRenderedFrame = output.scene.frame_number;
    window.dispatchEvent(new CustomEvent("aria-render-frame", { detail: output.scene }));

    for (const command of output.runtime) {
      if (command.kind === "save") {
        const envelope = runtime.save_envelope_json(BigInt(Date.now()));
        await saves.put(bundle.save_namespace, command.slot, envelope);
      } else if (command.kind === "load") {
        const generations = await saves.generations(bundle.save_namespace, command.slot);
        for (const generation of generations) {
          try {
            // Validate in an isolated Runtime first. The live Runtime must be
            // unchanged if this IndexedDB generation is corrupt, while a
            // valid restore follows Native's order: stop old device sources,
            // restore VM state, then consume the restored VM audio commands
            // on the following frame.
            const probe = new WebRuntime(
              bytecode,
              bundle.logical_width,
              bundle.logical_height,
            );
            probe.restore_envelope_json(generation.payload);
            audio.stopAll();
            runtime.restore_envelope_json(generation.payload);
            break;
          } catch (error) {
            console.warn("Skipping invalid save generation", generation.generation, error);
          }
        }
      }
    }
    if (!output.halted) scheduleFrame();
    else setStatus("ゲームを終了しました。");
  }

  function scheduleFrame() {
    requestAnimationFrame((now) => {
      frame(now).catch((error) => {
        console.error(error);
        setStatus(`起動失敗: ${error.message || error}`);
      });
    });
  }
  scheduleFrame();
}

boot().catch((error) => {
  console.error(error);
  setStatus(`起動失敗: ${error.message || error}`);
});
