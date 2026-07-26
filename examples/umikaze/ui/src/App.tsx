import { useEffect, useId, useRef, useState } from "react";
import {
  Button,
  Dialog,
  Heading,
  Modal,
  ModalOverlay,
} from "react-aria-components";
import {
  actionEnabled,
  routeName,
  type ActionView,
  type AriaStepOutput,
  type ChoiceView,
  type UiIntent,
  type UiViewModel,
} from "@aria/ui-sdk";

import { languageNames, localeFor, strings } from "./copy";
import { bootPresentation, type PresentationRuntime, type SaveSlotSummary } from "./runtime";
import { chapterPreviewByLabel } from "#chapter-preview";
import {
  chapterFallbackSources,
  gallerySources,
  sceneAssetByTone,
  sceneSources,
  stagePhotoByKind,
} from "#scene-assets";
import type { ChapterPreviewRecord } from "./chapter-preview.types";
import "./app.css";
import "./stage.css";

const isDemoEdition = import.meta.env.VITE_UMIKAZE_EDITION === "demo";

type Dispatch = (intent: UiIntent) => void;

function isOverlayRoute(route: string, view?: UiViewModel | null): boolean {
  // `chapter_select` first presents a line of narration on the reading
  // surface, then opens the actual chapter sheet after advance. Its route is
  // shared, but only the latter is a modal layer.
  if (route === "chapter_select" && view?.dialogue && view.choices.length === 0) return false;
  return ["pause", "save", "load", "settings", "backlog", "chapter_select", "gallery", "confirm"].includes(route);
}

function isInteractiveTarget(target: EventTarget | null): boolean {
  const element = target instanceof Element ? target : null;
  return Boolean(element?.closest("button, input, textarea, select, [contenteditable=true], [data-aria-action]"));
}

const sceneToneByColor: Record<string, string> = {
  "16,43,56": "tide",
  "40,75,89": "school",
  "31,59,77": "motion",
  "61,70,85": "ward",
  "36,78,90": "blue",
  "57,72,87": "rain",
  "23,37,59": "blue",
  "49,85,101": "shore",
  "55,90,97": "blue",
  "109,107,87": "clear",
  "5,7,11": "blackout",
  "222,215,201": "whiteout",
  "34,60,76": "station",
  "62,76,92": "hotel",
  "38,62,71": "harbor",
  "23,45,66": "night",
  "82,107,124": "platform",
  "83,109,120": "mist",
  "118,110,97": "rail-sunset",
  "15,47,57": "city",
  "48,74,87": "rain-city",
  "30,41,55": "bridge",
  "47,69,85": "passage",
  "22,61,74": "shore",
  "64,84,105": "night",
};

// Day cards have their own environmental reading.  A chapter opens on the
// weather of *that* day rather than inheriting a generic menu photograph.
// The story stays declarative: its first choice is the source of this tiny
// presentation hint, so saving on a card and restoring it remains exact.
const dayCardToneByHeading: Record<string, string> = {
  PROLOGUE: "ward",
  "DAY 1": "station",
  "DAY 2": "rain",
  "DAY 3": "hotel",
  "DAY 4": "blue",
  "DAY 5": "rain",
  "DAY 6": "clear",
  "DAY 7": "shore",
  "DAY 8": "harbor",
  "DAY 9": "night",
  "DAY 10": "blue",
  "DAY 14": "passage",
  EPILOGUE: "clear",
};

const dayCardThemeByHeading: Record<string, string> = {
  PROLOGUE: "ward",
  "DAY 1": "departure",
  "DAY 2": "rain",
  "DAY 3": "rail",
  "DAY 4": "shore",
  "DAY 5": "rain",
  "DAY 6": "clear",
  "DAY 7": "island",
  "DAY 8": "harbor",
  "DAY 9": "north",
  "DAY 10": "terminus",
  "DAY 14": "shore",
  EPILOGUE: "spring",
};

function dayCardHeading(view: UiViewModel | null | undefined): string | null {
  if (!view || routeName(view.route) !== "day_card") return null;
  return view.choices[0]?.label.split("\n", 1)[0]?.trim() || null;
}

function dayCardThemeFor(view: UiViewModel | null | undefined): string {
  const heading = dayCardHeading(view);
  return heading ? dayCardThemeByHeading[heading] || "record" : "record";
}

function toneForScene(output: AriaStepOutput | null): string {
  if (!output) return "loading";
  const route = routeName(output.view.route);
  if (route === "setup" || route === "title") return "title";
  if (route === "demo_end") return "shore";
  if (route === "day_card") {
    const heading = dayCardHeading(output.view);
    return heading ? dayCardToneByHeading[heading] || "tide" : "tide";
  }
  const scene = output.scene as unknown as { commands?: unknown[] };
  const background = scene.commands?.find((command) => (
    Boolean(command)
    && typeof command === "object"
    && (command as { id?: unknown }).id === "scene.background"
  )) as { kind?: unknown; color?: { red?: unknown; green?: unknown; blue?: unknown } } | undefined;
  if (background?.kind === "rectangle" && background.color) {
    const color = background.color;
    const key = [color.red, color.green, color.blue].map(Number).join(",");
    return sceneToneByColor[key] || "tide";
  }
  return route === "chapter_select" ? "tide" : "night";
}

/**
 * The scene is a place before it is an interface.  The photos are original
 * project art; this component only decides which one belongs to the current
 * story state, without inventing another graphic language over it.
 */
function ScenePhotograph({ output, transform }: { output: AriaStepOutput | null; transform?: string }) {
  const route = output ? routeName(output.view.route) : "loading";
  const tone = toneForScene(output);
  const asset = sceneAssetByTone[tone] || sceneAssetByTone.coast;
  if (!asset.source) {
    return (
      <div
        key={`${route}-${tone}`}
        className={`scene-photograph scene-photograph--${asset.name} scene-photograph--tone-${tone}`}
        style={{ backgroundColor: asset.solid || "#0b1419", ...(transform ? { transform } : {}) }}
        aria-hidden="true"
      />
    );
  }
  return (
    <div
      key={`${route}-${tone}`}
      className={`scene-photograph scene-photograph--${asset.name} scene-photograph--tone-${tone}`}
      style={transform ? { transform } : undefined}
      aria-hidden="true"
    >
      <img src={asset.source} alt="" decoding="async" />
    </div>
  );
}

type SceneDirectionOverlay = {
  key: string;
  color: string;
};

type SceneDirection = {
  transform?: string;
  overlays: SceneDirectionOverlay[];
};

const emptySceneDirection: SceneDirection = { overlays: [] };

function finiteNumber(value: unknown, fallback = 0): number {
  const number = Number(value);
  return Number.isFinite(number) ? number : fallback;
}

function clampedNumber(value: unknown, minimum: number, maximum: number): number {
  return Math.min(maximum, Math.max(minimum, finiteNumber(value)));
}

/**
 * The semantic scene contract keeps effects intentionally opaque to UI
 * components. UmiKaze owns the visible stage, so it reads only the tiny,
 * stable effect subset here. The calculation mirrors the native renderer;
 * importantly, it has no timer of its own and therefore leaves still frames
 * completely still.
 */
function directionForScene(output: AriaStepOutput | null, reducedMotion: boolean): SceneDirection {
  const rawScene = output?.scene as unknown as { effects?: unknown } | undefined;
  if (!Array.isArray(rawScene?.effects)) return emptySceneDirection;

  let shakeX = 0;
  let shakeY = 0;
  const overlays: SceneDirectionOverlay[] = [];

  rawScene.effects.forEach((rawEffect, index) => {
    if (!rawEffect || typeof rawEffect !== "object") return;
    const effect = rawEffect as {
      kind?: unknown;
      color?: { red?: unknown; green?: unknown; blue?: unknown };
      opacity?: unknown;
      amplitude?: unknown;
      progress?: unknown;
    };
    const progress = clampedNumber(effect.progress, 0, 1);

    if (effect.kind === "shake") {
      if (reducedMotion) return;
      const amplitude = Math.max(0, finiteNumber(effect.amplitude));
      const fade = 1 - progress;
      const phase = progress * Math.PI * 2 * 3;
      shakeX += amplitude * Math.sin(phase) * fade;
      shakeY += amplitude * Math.cos(phase * 1.37) * fade;
      return;
    }

    if ((effect.kind !== "tint" && effect.kind !== "flash") || !effect.color) return;
    const opacity = clampedNumber(effect.opacity, 0, 255) / 255 * (1 - progress);
    if (opacity <= 0) return;
    const red = Math.round(clampedNumber(effect.color.red, 0, 255));
    const green = Math.round(clampedNumber(effect.color.green, 0, 255));
    const blue = Math.round(clampedNumber(effect.color.blue, 0, 255));
    overlays.push({
      key: `${String(effect.kind)}-${index}`,
      color: `rgb(${red} ${green} ${blue} / ${opacity})`,
    });
  });

  if (reducedMotion || (Math.abs(shakeX) < 0.01 && Math.abs(shakeY) < 0.01)) return { overlays };
  // A small scale absorbs the edge of the shake, so a physical jolt never
  // exposes a browser-coloured seam around the still photograph.
  return { transform: `translate3d(${shakeX.toFixed(2)}px, ${shakeY.toFixed(2)}px, 0) scale(1.018)`, overlays };
}

function SceneDirectionLayer({ overlays }: { overlays: SceneDirectionOverlay[] }) {
  if (overlays.length === 0) return null;
  return (
    <div className="scene-direction-layer" aria-hidden="true">
      {overlays.map((overlay) => (
        <div key={overlay.key} className="scene-direction-effect" style={{ backgroundColor: overlay.color }} />
      ))}
    </div>
  );
}

function ActionButton({
  label,
  id,
  active = false,
  disabled = false,
  onAction,
  className = "action-button",
}: {
  label: string;
  id: string;
  active?: boolean;
  disabled?: boolean;
  onAction: (id: string) => void;
  className?: string;
}) {
  return (
    <Button
      className={`${className}${active ? " is-active" : ""}`}
      data-aria-focusable
      data-aria-action={id}
      isDisabled={disabled}
      onPress={() => onAction(id)}
    >
      {label}
    </Button>
  );
}

function ChoiceButton({ choice, onAction, wide = false }: {
  choice: ChoiceView;
  onAction: (id: string) => void;
  wide?: boolean;
}) {
  return (
    <Button
      className={`choice-button${choice.selected ? " is-selected" : ""}${wide ? " is-wide" : ""}`}
      data-aria-focusable
      data-aria-action={choice.id}
      onPress={() => onAction(choice.id)}
    >
      <span>{choice.label}</span>
    </Button>
  );
}

type FocusMenuItem = {
  id: string;
  label: string;
  description: string;
  active?: boolean;
  disabled?: boolean;
  accessibleLabel?: string;
};

/**
 * Static visual material for every non-reading screen.  It is intentionally
 * CSS-only: the game already owns the photograph underneath, so this only
 * supplies the aged signal and margin light that make a system screen feel
 * like part of the same record rather than a web overlay.
 */
function StageBackdrop({ kind = "record" }: { kind?: string }) {
  const isTitle = kind === "title";
  const photo = stagePhotoByKind[kind] || stagePhotoByKind.record;
  return (
    <div className={`record-stage-backdrop record-stage-backdrop--${kind}`} aria-hidden="true">
      <img className={`record-stage-photograph record-stage-photograph--${photo.name}`} src={photo.source} alt="" decoding="async" />
      {isTitle && (
        <div className="record-stage-fragments">
          <span className="record-stage-fragment record-stage-fragment--tractatus-silence" lang="de">
            7&nbsp; Wovon man nicht sprechen kann, darüber muß man schweigen.
          </span>
          <span className="record-stage-fragment record-stage-fragment--yodaka" lang="ja">
            よだかは、実にみにくい鳥です。
          </span>
          <span className="record-stage-fragment record-stage-fragment--galaxy" lang="ja">
            ではみなさんは、そういうふうに川だと云われたり、乳の流れたあとだと云われたりしていたこのぼんやりと白いものがほんとうは何かご承知ですか。
          </span>
        </div>
      )}
      <span className="record-stage-signal record-stage-signal--one" />
    </div>
  );
}

function FocusDescription({ id, description }: { id: string; description: string }) {
  return <p id={id} className="focus-menu-description" aria-live="off">{description}</p>;
}

/**
 * A deterministic vertical command list. Pointer movement merely previews a
 * command; focus, Enter, touch, and the gamepad still share the same button
 * path. The title, first-light, and transparent RMenu variants can attach
 * their note directly below the focused command, so an explanation never
 * becomes a separate web-like panel.
 */
function FocusMenu({
  label,
  items,
  onAction,
  className = "",
  initialFocusId,
  descriptionPlacement = "after-list",
}: {
  label: string;
  items: FocusMenuItem[];
  onAction: (id: string) => void;
  className?: string;
  initialFocusId?: string;
  descriptionPlacement?: "after-list" | "under-focused-item";
}) {
  const descriptionId = useId();
  const menuRef = useRef<HTMLElement | null>(null);
  const firstAvailable = items.find((item) => !item.disabled)?.id ?? "";
  const initialFocusableId = items.some((item) => item.id === initialFocusId && !item.disabled)
    ? initialFocusId
    : firstAvailable;
  const [focusedActionId, setFocusedActionId] = useState(initialFocusId ?? firstAvailable);
  const focused = items.find((item) => item.id === focusedActionId)
    ?? items.find((item) => !item.disabled)
    ?? items[0];

  useEffect(() => {
    if (items.some((item) => item.id === focusedActionId && !item.disabled)) return;
    setFocusedActionId(initialFocusId && items.some((item) => item.id === initialFocusId && !item.disabled)
      ? initialFocusId
      : firstAvailable);
  }, [firstAvailable, focusedActionId, initialFocusId, items]);

  // Opening a command surface must make it immediately usable with a d-pad
  // or arrow keys. React Aria's focus scope intentionally has no opinion
  // about our visual command order, so place focus on the deterministic first
  // command exactly once for each newly mounted (or reconfigured) menu.
  // This is deliberately not driven by hover/state updates: pointer preview
  // remains a preview and never steals a keyboard player's focus.
  useEffect(() => {
    if (!initialFocusableId) return;
    const target = [...(menuRef.current?.querySelectorAll<HTMLButtonElement>("[data-stage-menu-item]") ?? [])]
      .find((control) => control.dataset.ariaAction === initialFocusableId && !control.disabled);
    target?.focus({ preventScroll: true });
  }, [initialFocusableId]);

  const moveFocus = (container: HTMLElement, direction: -1 | 1) => {
    const controls = [...container.querySelectorAll<HTMLButtonElement>("[data-stage-menu-item]")]
      .filter((control) => !control.disabled);
    if (!controls.length) return;
    const activeIndex = controls.findIndex((control) => control === document.activeElement);
    const next = activeIndex < 0
      ? (direction > 0 ? 0 : controls.length - 1)
      : (activeIndex + direction + controls.length) % controls.length;
    controls[next]?.focus({ preventScroll: true });
  };

  return (
    <nav
      ref={menuRef}
      className={`focus-menu ${className}`.trim()}
      aria-label={label}
      onKeyDown={(event) => {
        if (event.key === "ArrowUp") {
          event.preventDefault();
          moveFocus(event.currentTarget, -1);
        }
        if (event.key === "ArrowDown") {
          event.preventDefault();
          moveFocus(event.currentTarget, 1);
        }
      }}
    >
      <div className="focus-menu-list">
        {items.map((item) => {
          const isFocused = item.id === focused?.id;
          const noteFollowsItem = descriptionPlacement === "under-focused-item";
          return (
            <Button
              key={item.id}
              className={`focus-menu-item${isFocused ? " is-focused" : ""}${item.active ? " is-active" : ""}`}
              data-aria-focusable
              data-aria-action={item.id}
              data-stage-menu-item
              aria-describedby={noteFollowsItem ? undefined : descriptionId}
              aria-label={item.accessibleLabel}
              isDisabled={item.disabled}
              onFocus={() => setFocusedActionId(item.id)}
              onPointerEnter={() => setFocusedActionId(item.id)}
              onPress={() => onAction(item.id)}
            >
              <span className="focus-menu-command">{item.label}</span>
              {item.active && <span className="focus-menu-state" aria-label="ON">ON</span>}
              {noteFollowsItem && <span className="focus-menu-inline-description">{item.description}</span>}
            </Button>
          );
        })}
      </div>
      {descriptionPlacement === "after-list" && <FocusDescription id={descriptionId} description={focused?.description ?? ""} />}
    </nav>
  );
}

function steppedValue(value: number, direction: -1 | 1, min: number, max: number, step: number) {
  const precision = Math.max(0, String(step).split(".")[1]?.length ?? 0);
  const index = Math.round((value - min) / step) + direction;
  return Number(Math.min(max, Math.max(min, min + index * step)).toFixed(precision));
}

/** A game-like, explicit left/value/right rail. No native browser controls. */
function SettingRail({
  label,
  value,
  min,
  max,
  step,
  valueLabel,
  onChange,
}: {
  label: string;
  value: number;
  min: number;
  max: number;
  step: number;
  valueLabel: string;
  onChange(value: number): void;
}) {
  const labelId = useId();
  const decrease = () => onChange(steppedValue(value, -1, min, max, step));
  const increase = () => onChange(steppedValue(value, 1, min, max, step));
  return (
    <div
      className="setting-rail"
      onKeyDown={(event) => {
        if (event.key === "ArrowLeft") {
          event.preventDefault();
          decrease();
        }
        if (event.key === "ArrowRight") {
          event.preventDefault();
          increase();
        }
      }}
    >
      <span id={labelId} className="setting-rail-label">{label}</span>
      <div className="setting-rail-controls" role="group" aria-labelledby={labelId}>
        <Button className="setting-rail-button" data-aria-focusable aria-label={`${label}: decrease`} onPress={decrease}>◀</Button>
        <output className="setting-rail-value" aria-live="off">{valueLabel}</output>
        <Button className="setting-rail-button" data-aria-focusable aria-label={`${label}: increase`} onPress={increase}>▶</Button>
      </div>
    </div>
  );
}

function BinarySetting({
  label,
  selected,
  onSelectedChange,
}: {
  label: string;
  selected: boolean;
  onSelectedChange(selected: boolean): void;
}) {
  const labelId = useId();
  return (
    <div className="binary-setting">
      <span id={labelId} className="setting-rail-label">{label}</span>
      <div className="binary-setting-controls" role="group" aria-labelledby={labelId}>
        <Button className="binary-setting-button" data-aria-focusable aria-pressed={!selected}
          onPress={() => onSelectedChange(false)}>OFF</Button>
        <Button className="binary-setting-button" data-aria-focusable aria-pressed={selected}
          onPress={() => onSelectedChange(true)}>ON</Button>
      </div>
    </div>
  );
}

function OverlaySheet({
  title,
  kicker,
  dismissLabel,
  children,
  onDismiss,
  variant = "side",
  surface,
}: {
  title: string;
  kicker?: string;
  dismissLabel: string;
  children: React.ReactNode;
  onDismiss: () => void;
  variant?: "side" | "chapter" | "confirm" | "archive";
  surface?: "save" | "load" | "settings" | "backlog" | "gallery";
}) {
  return (
    <ModalOverlay
      className={`sheet-overlay sheet-overlay--${variant}`}
      isOpen
      isDismissable
      onOpenChange={(open) => { if (!open) onDismiss(); }}
    >
      <Modal className={`sheet-modal sheet-modal--${variant}${surface ? ` sheet-modal--${surface}` : ""}`}>
        <Dialog className={`sheet stage-sheet sheet--${variant}${surface ? ` sheet--${surface}` : ""}`} aria-label={title}>
          <StageBackdrop kind={surface ?? variant} />
          <div className="stage-sheet-content">
            <header className="sheet-header">
              <div className="sheet-heading">
                {kicker && <p className="sheet-kicker">{kicker}</p>}
                <Heading slot="title">{title}</Heading>
              </div>
              <Button className="stage-close" aria-label={dismissLabel} data-aria-focusable onPress={onDismiss}>CLOSE</Button>
            </header>
            <div className="sheet-tide-line" aria-hidden="true" />
            {children}
          </div>
        </Dialog>
      </Modal>
    </ModalOverlay>
  );
}

function SettingsSheet({ view, copy, dispatch }: {
  view: UiViewModel;
  copy: ReturnType<typeof strings>;
  dispatch: Dispatch;
}) {
  type SettingsSection = "text" | "sound" | "display" | "system";
  const [section, setSection] = useState<SettingsSection>("text");
  const set = (name: string, value: number) => dispatch({ kind: "set_setting", name, value });
  const toggle = (name: string) => dispatch({ kind: "toggle_setting", name });
  const setBoolean = (name: string, current: boolean, next: boolean) => {
    if (current !== next) toggle(name);
  };
  const sections: Array<{ id: SettingsSection; label: string; description: string }> = [
    { id: "text", label: "TEXT", description: copy.readingControls },
    { id: "sound", label: "SOUND", description: copy.sound },
    { id: "display", label: "DISPLAY", description: copy.display },
    { id: "system", label: "SYSTEM", description: copy.records },
  ];
  const selectedSection = sections.find((item) => item.id === section) ?? sections[0];
  return (
    <OverlaySheet title="CONFIG" kicker={copy.settings} dismissLabel={copy.close} variant="archive" surface="settings" onDismiss={() => dispatch({ kind: "dismiss" })}>
      <div className="settings-stage">
        <nav className="settings-index" aria-label={copy.settings}>
          {sections.map((item) => (
            <Button key={item.id} className={`settings-index-item${section === item.id ? " is-selected" : ""}`}
              data-aria-focusable aria-pressed={section === item.id} onPress={() => setSection(item.id)}>
              {item.label}
            </Button>
          ))}
        </nav>
        <section className="settings-deck" aria-label={selectedSection.description}>
          <header className="settings-deck-header">
            <h3>{selectedSection.description}</h3>
          </header>
          {section === "text" && (
            <div className="settings-rails">
              <SettingRail label={copy.textSpeed} value={view.settings.text_speed_ms} min={0} max={120} step={4}
                valueLabel={copy.valueMs(view.settings.text_speed_ms)} onChange={(value) => set("text_speed_ms", value)} />
              <SettingRail label={copy.autoDelay} value={view.settings.auto_delay_ms} min={100} max={3000} step={100}
                valueLabel={copy.valueMs(view.settings.auto_delay_ms)} onChange={(value) => set("auto_delay_ms", value)} />
              <SettingRail label={copy.textSize} value={view.settings.text_scale} min={0.85} max={1.35} step={0.05}
                valueLabel={copy.valuePercent(view.settings.text_scale)} onChange={(value) => set("text_scale", value)} />
              <SettingRail label={copy.subtitleOpacity} value={view.settings.text_opacity} min={0.72} max={1} step={0.04}
                valueLabel={copy.valuePercent(view.settings.text_opacity)} onChange={(value) => set("text_opacity", value)} />
            </div>
          )}
          {section === "sound" && (
            <div className="settings-rails">
              <SettingRail label={copy.music} value={view.settings.bgm_volume} min={0} max={1} step={0.05}
                valueLabel={copy.valuePercent(view.settings.bgm_volume)} onChange={(value) => set("bgm_volume", value)} />
              <SettingRail label={copy.effects} value={view.settings.sound_effect_volume} min={0} max={1} step={0.05}
                valueLabel={copy.valuePercent(view.settings.sound_effect_volume)} onChange={(value) => set("sound_effect_volume", value)} />
              <SettingRail label={copy.voice} value={view.settings.voice_volume} min={0} max={1} step={0.05}
                valueLabel={copy.valuePercent(view.settings.voice_volume)} onChange={(value) => set("voice_volume", value)} />
            </div>
          )}
          {section === "display" && (
            <div className="settings-rails">
              <BinarySetting label={copy.fullscreen} selected={view.settings.fullscreen}
                onSelectedChange={(next) => setBoolean("fullscreen", view.settings.fullscreen, next)} />
              <BinarySetting label={copy.contrast} selected={view.settings.high_contrast}
                onSelectedChange={(next) => setBoolean("high_contrast", view.settings.high_contrast, next)} />
              <BinarySetting label={copy.reducedMotion} selected={view.settings.reduced_motion}
                onSelectedChange={(next) => setBoolean("reduced_motion", view.settings.reduced_motion, next)} />
              <BinarySetting label={copy.stageEffects} selected={view.settings.stage_effects}
                onSelectedChange={(next) => setBoolean("stage_effects", view.settings.stage_effects, next)} />
            </div>
          )}
          {section === "system" && (
            <div className="settings-rails">
              <BinarySetting label={copy.skipUnread} selected={view.settings.skip_unread}
                onSelectedChange={(next) => setBoolean("skip_unread", view.settings.skip_unread, next)} />
            </div>
          )}
        </section>
      </div>
    </OverlaySheet>
  );
}

function RMenu({ view, copy, onAction, dispatch }: {
  view: UiViewModel;
  copy: ReturnType<typeof strings>;
  onAction: (id: string) => void;
  dispatch: Dispatch;
}) {
  const action = (id: string) => view.actions.find((item) => item.id === id);
  const items: FocusMenuItem[] = [
    { id: "dismiss", label: "RESUME", description: copy.menuDescription.resume },
    { id: "menu.auto", label: "AUTO", description: copy.menuDescription.auto, active: action("menu.auto")?.active, disabled: !action("menu.auto")?.enabled },
    { id: "menu.skip", label: "SKIP", description: copy.menuDescription.skip, active: action("menu.skip")?.active, disabled: !action("menu.skip")?.enabled },
    { id: "menu.backlog", label: "LOG", description: copy.menuDescription.log, disabled: !action("menu.backlog")?.enabled },
    { id: "menu.save", label: "SAVE", description: copy.menuDescription.save, disabled: !action("menu.save")?.enabled },
    { id: "menu.load", label: "LOAD", description: copy.menuDescription.load, disabled: !action("menu.load")?.enabled },
    { id: "menu.gallery", label: "EXTRA", description: copy.menuDescription.extra, disabled: !action("menu.gallery")?.enabled },
    { id: "menu.settings", label: "CONFIG", description: copy.menuDescription.config, disabled: !action("menu.settings")?.enabled },
    { id: "menu.reset", label: "TITLE", description: copy.menuDescription.title, disabled: !action("menu.reset")?.enabled },
    { id: "menu.quit", label: "EXIT", description: copy.menuDescription.exit, disabled: !action("menu.quit")?.enabled },
  ];
  return (
    <ModalOverlay className="rmenu-overlay" isOpen isDismissable onOpenChange={(open) => { if (!open) dispatch({ kind: "dismiss" }); }}>
      <Modal className="rmenu-modal">
        <Dialog className="pause-ledger stage-rmenu" aria-label={copy.menu}>
          <FocusMenu label={copy.menu} items={items} initialFocusId="dismiss" descriptionPlacement="under-focused-item" className="rmenu-command-list"
            onAction={(id) => {
              if (id === "dismiss") dispatch({ kind: "dismiss" });
              else onAction(id);
            }} />
        </Dialog>
      </Modal>
    </ModalOverlay>
  );
}

function ConfirmationSheet({ view, copy, onAction, dispatch }: {
  view: UiViewModel;
  copy: ReturnType<typeof strings>;
  onAction: (id: string) => void;
  dispatch: Dispatch;
}) {
  const action = view.confirmation?.action;
  const message = action === "quit"
    ? copy.confirmQuit
    : action === "resume_backlog"
      ? copy.confirmResume
      : copy.confirmReset;
  const resume = action === "resume_backlog";
  return (
    <OverlaySheet title="CONFIRM" kicker={copy.confirm} dismissLabel={copy.close} variant="confirm" onDismiss={() => dispatch({ kind: "activate", id: "confirm.cancel" })}>
      <p className="confirmation-message">{message}</p>
      <div className="confirmation-actions">
        <ActionButton id="confirm.accept" label={resume ? copy.ok : copy.proceed} onAction={onAction} className="confirmation-accept" />
        <ActionButton id="confirm.cancel" label={resume ? copy.ng : copy.cancel} onAction={onAction} className="confirmation-cancel" />
      </div>
    </OverlaySheet>
  );
}

function SaveLoadSheet({ kind, view, copy, onAction, dispatch, saveSlots }: {
  kind: "save" | "load";
  view: UiViewModel;
  copy: ReturnType<typeof strings>;
  onAction: (id: string) => void;
  dispatch: Dispatch;
  saveSlots: SaveSlotSummary[];
}) {
  const label = kind === "save" ? "SAVE" : "LOAD";
  const records = new Map(saveSlots.map((record) => [record.slot, record]));
  const recordDescription = (record: SaveSlotSummary | undefined) => {
    if (!record) return null;
    const timestamp = record.timestampMs
      ? new Intl.DateTimeFormat(localeFor(view.game.locale), { dateStyle: "medium", timeStyle: "short" }).format(new Date(record.timestampMs))
      : null;
    return [record.speaker, record.excerpt, timestamp]
      .filter((value): value is string => Boolean(value))
      .join(" · ") || copy.previousRecord;
  };
  return (
    <OverlaySheet title={label} kicker={kind === "save" ? copy.save : copy.load} dismissLabel={copy.close} variant="archive" surface={kind} onDismiss={() => dispatch({ kind: "dismiss" })}>
      <p className="sheet-intro">{kind === "save" ? copy.saveLead : copy.loadLead}</p>
      <div className="save-slots">
        {Array.from({ length: 10 }, (_, index) => index + 1).map((slot) => {
          const id = `${kind}.slot.${slot}`;
          const slotLabel = kind === "save" ? copy.saveSlot(slot) : copy.loadSlot(slot);
          const record = records.get(slot);
          const description = recordDescription(record) || (kind === "load" ? copy.emptyRecord : slotLabel);
          const descriptionId = `record-slot-${kind}-${slot}`;
          return (
            <Button key={id} data-aria-focusable data-aria-action={id} className="record-slot"
              aria-label={slotLabel} aria-describedby={descriptionId}
              isDisabled={!actionEnabled(view, id) || (kind === "load" && !record)} onPress={() => onAction(id)}>
              <span className="record-slot-index">{copy.recordIndex(slot)}</span>
              <span className="record-slot-action">{record?.speaker || (kind === "save" ? copy.writeRecord : copy.openRecord)}</span>
              <span id={descriptionId} className="record-slot-label">{description}</span>
            </Button>
          );
        })}
      </div>
    </OverlaySheet>
  );
}

function BacklogSheet({ view, copy, onAction, dispatch }: {
  view: UiViewModel;
  copy: ReturnType<typeof strings>;
  onAction: (id: string) => void;
  dispatch: Dispatch;
}) {
  const list = useRef<HTMLDivElement>(null);
  const lastScrollTop = useRef(0);
  const backlogRowHeight = 104;
  const scrollPage = (element: HTMLDivElement, direction: -1 | 1) => {
    // Keep log navigation in whole readable chunks.  The list owns its own
    // scroll frame, so this does not move the surrounding system sheet.
    const pageHeight = Math.max(
      backlogRowHeight,
      Math.floor(element.clientHeight / backlogRowHeight) * backlogRowHeight,
    );
    element.scrollBy(0, pageHeight * direction);
  };
  useEffect(() => {
    const element = list.current;
    if (!element) return;
    const rowHeight = backlogRowHeight;
    const target = view.backlog_start * rowHeight;
    if (Math.abs(element.scrollTop - target) > rowHeight) element.scrollTop = target;
    lastScrollTop.current = element.scrollTop;
  }, [view.backlog_start, backlogRowHeight]);

  return (
    <OverlaySheet title="LOG" kicker={copy.history} dismissLabel={copy.close} variant="archive" surface="backlog" onDismiss={() => dispatch({ kind: "dismiss" })}>
      <div
        ref={list}
        className="backlog-list"
        role="region"
        tabIndex={0}
        aria-label={copy.history}
        aria-keyshortcuts="PageUp PageDown Home End"
        onKeyDown={(event) => {
          if (event.key === "PageDown") {
            event.preventDefault();
            scrollPage(event.currentTarget, 1);
          } else if (event.key === "PageUp") {
            event.preventDefault();
            scrollPage(event.currentTarget, -1);
          } else if (event.key === "Home") {
            event.preventDefault();
            event.currentTarget.scrollTo(0, 0);
          } else if (event.key === "End") {
            event.preventDefault();
            event.currentTarget.scrollTo(0, event.currentTarget.scrollHeight);
          }
        }}
        onScroll={(event) => {
          const top = event.currentTarget.scrollTop;
          const delta = top - lastScrollTop.current;
          lastScrollTop.current = top;
          // Core's semantic scroll unit is 48px (the native wheel step),
          // while this virtual list uses fixed 104px rows. Convert rather
          // than sending physical pixels so the window advances one entry
          // per visible row on every host.
          if (Math.abs(delta) >= 1) dispatch({ kind: "scroll", region: "backlog", delta_y: delta * 48 / backlogRowHeight });
        }}
      >
        {view.backlog_total === 0 && <p className="empty-state">{copy.noEntries}</p>}
        {view.backlog_total > 0 && (
          <div className="backlog-virtual" style={{ height: `${Math.max(view.backlog_total * backlogRowHeight, 1)}px` }}>
            <div className="backlog-window" style={{ transform: `translateY(${view.backlog_start * backlogRowHeight}px)` }}>
              {view.backlog.map((entry, index) => {
          const id = `backlog:${entry.id}`;
          return (
            <Button key={entry.id} data-aria-focusable data-aria-action={id}
              className={`backlog-entry${entry.selected ? " is-selected" : ""}`} onPress={() => onAction(id)}
              aria-posinset={view.backlog_start + index + 1} aria-setsize={view.backlog_total}>
              <span className="backlog-index">{String(view.backlog_start + index + 1).padStart(2, "0")}</span>
              <span className="backlog-copy">
                {entry.speaker && <span className="backlog-speaker">{entry.speaker}</span>}
                <span className="backlog-text">{entry.text}</span>
              </span>
            </Button>
          );
              })}
            </div>
          </div>
        )}
      </div>
    </OverlaySheet>
  );
}

function ChapterSheet({ view, copy, onAction, dispatch }: {
  view: UiViewModel;
  copy: ReturnType<typeof strings>;
  onAction: (id: string) => void;
  dispatch: Dispatch;
}) {
  type ChapterCard = {
    id: string;
    label: string;
    description: string;
    date: string;
    preview?: ChapterPreviewRecord;
    unlocked: boolean;
    selected: boolean;
  };
  const cards: ChapterCard[] = view.choices.length ? view.choices.map((choice) => {
    const preview = chapterPreviewByLabel[choice.label.trim()];
    return {
      id: choice.id,
      label: choice.label,
      description: preview?.synopsis ?? "",
      date: preview?.date ?? "",
      preview,
      unlocked: true,
      selected: choice.selected,
    };
  }) : view.chapters.map((chapter) => {
    const label = chapter.title || chapter.id;
    const preview = chapterPreviewByLabel[label.trim()];
    return {
      id: `chapter:${chapter.id}`,
      label,
      description: preview?.synopsis || chapter.description,
      date: preview?.date ?? "",
      preview,
      unlocked: chapter.unlocked,
      selected: false,
    };
  });
  const initialPreviewId = cards.find((card) => card.selected)?.id
    ?? cards.find((card) => card.unlocked)?.id
    ?? cards[0]?.id
    ?? "";
  const [previewChapterId, setPreviewChapterId] = useState(initialPreviewId);
  useEffect(() => {
    if (!cards.some((card) => card.id === previewChapterId)) setPreviewChapterId(initialPreviewId);
  }, [cards, initialPreviewId, previewChapterId]);
  const featured = cards.find((card) => card.id === previewChapterId) ?? cards.find((card) => card.id === initialPreviewId);
  const featuredIndex = Math.max(0, cards.findIndex((card) => card.id === featured?.id));
  const previewSource = (featured?.preview && sceneSources[featured.preview.scene])
    ?? chapterFallbackSources[featuredIndex % chapterFallbackSources.length];
  const moveIndexFocus = (container: HTMLElement, direction: -1 | 1) => {
    const controls = [...container.querySelectorAll<HTMLButtonElement>("[data-chapter-index-item]")]
      .filter((control) => !control.disabled);
    if (!controls.length) return;
    const activeIndex = controls.findIndex((control) => control === document.activeElement);
    const next = activeIndex < 0
      ? (direction > 0 ? 0 : controls.length - 1)
      : (activeIndex + direction + controls.length) % controls.length;
    controls[next]?.focus({ preventScroll: true });
  };
  return (
    <OverlaySheet title="CHAPTERS" kicker={copy.chapters} dismissLabel={copy.close} variant="chapter" onDismiss={() => dispatch({ kind: "dismiss" })}>
      <div className="chapter-stage">
        {featured && (
          <section className="chapter-preview" aria-label={featured.unlocked ? featured.label : copy.locked}>
            <img className="chapter-preview-image" src={previewSource} alt="" />
            <div className="chapter-preview-record">
              {featured.unlocked && featured.date && <p className="chapter-preview-date">{featured.date}</p>}
              <h3>{featured.unlocked ? featured.label : "SEALED"}</h3>
              {featured.unlocked && featured.description && <p className="chapter-preview-description">{featured.description}</p>}
              {!featured.unlocked && <p className="chapter-preview-description">{copy.locked}</p>}
            </div>
          </section>
        )}
        <nav className="chapter-index-menu" aria-label={copy.chapters} onKeyDown={(event) => {
          if (event.key === "ArrowUp") {
            event.preventDefault();
            moveIndexFocus(event.currentTarget, -1);
          }
          if (event.key === "ArrowDown") {
            event.preventDefault();
            moveIndexFocus(event.currentTarget, 1);
          }
        }}>
          {cards.map((card) => (
            <Button key={card.id} data-aria-focusable data-aria-action={card.id} data-chapter-index-item
              className={`chapter-index-row${card.unlocked ? "" : " is-locked"}${card.id === featured?.id ? " is-preview" : ""}`}
              aria-label={card.unlocked ? card.label : copy.locked}
              isDisabled={!card.unlocked} onFocus={() => setPreviewChapterId(card.id)} onPointerEnter={() => setPreviewChapterId(card.id)}
              onPress={() => onAction(card.id)}>
              <span className="chapter-index-name">{card.unlocked ? card.label : "SEALED"}</span>
              <span className="chapter-index-rule" aria-hidden="true" />
            </Button>
          ))}
        </nav>
      </div>
    </OverlaySheet>
  );
}

function GallerySheet({ view, copy, onAction, dispatch }: {
  view: UiViewModel;
  copy: ReturnType<typeof strings>;
  onAction: (id: string) => void;
  dispatch: Dispatch;
}) {
  const swipeStart = useRef<{ x: number; y: number } | null>(null);
  const selectedIndex = Math.max(0, view.gallery.findIndex((item) => item.id === view.gallery_viewer));
  const selected = view.gallery[selectedIndex];
  if (view.gallery_viewer && selected) {
    return (
      <ModalOverlay className="gallery-viewer-overlay" isOpen isDismissable onOpenChange={(open) => {
        if (!open) onAction("gallery.close");
      }}>
        <Modal className="gallery-viewer-modal">
          <Dialog className="gallery-viewer" aria-label={copy.memory(selectedIndex + 1)}
            onPointerDown={(event) => { swipeStart.current = { x: event.clientX, y: event.clientY }; }}
            onPointerUp={(event) => {
              const start = swipeStart.current;
              swipeStart.current = null;
              if (!start) return;
              const deltaX = event.clientX - start.x;
              const deltaY = event.clientY - start.y;
              if (Math.abs(deltaX) >= 48 && Math.abs(deltaX) > Math.abs(deltaY)) {
                onAction(deltaX < 0 ? "gallery.next" : "gallery.previous");
              }
            }}>
            <img className="gallery-viewer-image" src={gallerySources[selectedIndex % gallerySources.length]} alt={copy.memory(selectedIndex + 1)} />
            <div className="gallery-viewer-shade" aria-hidden="true" />
            <header className="gallery-viewer-header">
              <span>{copy.memory(selectedIndex + 1)}</span>
              <Button className="icon-button" data-aria-focusable data-aria-action="gallery.close" aria-label={copy.close} onPress={() => onAction("gallery.close")}>×</Button>
            </header>
            <div className="gallery-viewer-controls" aria-label={copy.gallery}>
              <ActionButton id="gallery.previous" label={copy.previousMemory} onAction={onAction} className="gallery-viewer-control" />
              <span aria-live="off">{String(selectedIndex + 1).padStart(2, "0")} / {String(view.gallery.length).padStart(2, "0")}</span>
              <ActionButton id="gallery.next" label={copy.nextMemory} onAction={onAction} className="gallery-viewer-control gallery-viewer-control--next" />
            </div>
          </Dialog>
        </Modal>
      </ModalOverlay>
    );
  }
  return (
    <OverlaySheet title="EXTRA" kicker={copy.gallery} dismissLabel={copy.close} variant="archive" surface="gallery" onDismiss={() => dispatch({ kind: "dismiss" })}>
      <div className="gallery-grid">
        {view.gallery.length === 0 && <p className="empty-state">{copy.noEntries}</p>}
        {view.gallery.map((item, index) => (
          <Button key={item.id} data-aria-focusable data-aria-action={`gallery:${item.id}`}
            className={`gallery-card gallery-card--${index % 3}${item.unlocked ? "" : " is-locked"}${item.selected ? " is-selected" : ""}`}
            aria-label={item.unlocked ? copy.memory(index + 1) : copy.locked}
            isDisabled={!item.unlocked || !actionEnabled(view, `gallery:${item.id}`)}
            onPress={() => onAction(`gallery:${item.id}`)}>
            <span className="gallery-image" aria-hidden="true" style={{ backgroundImage: `linear-gradient(180deg, rgb(5 16 22 / 8%), rgb(5 16 22 / 64%)), url(${gallerySources[index % gallerySources.length]})` }} />
            <span className="gallery-index">{String(index + 1).padStart(2, "0")}</span>
            <span className="gallery-label">{item.unlocked ? copy.memory(index + 1) : copy.locked}</span>
          </Button>
        ))}
      </div>
    </OverlaySheet>
  );
}

function Dialogue({ view, copy, onAction, chromeVisible, onRevealChrome }: {
  view: UiViewModel;
  copy: ReturnType<typeof strings>;
  onAction: (id: string) => void;
  chromeVisible: boolean;
  onRevealChrome: () => void;
}) {
  const dialogue = view.dialogue;
  const completedAnnouncement = dialogue?.complete
    ? [dialogue.speaker, dialogue.full_page_text].filter(Boolean).join(" ")
    : "";
  const modeMark = view.auto_mode === "on" ? copy.auto : view.skip_mode !== "off" ? copy.skip : null;
  return (
    <>
      <header className={`quiet-chrome${chromeVisible ? " is-visible" : ""}`} onPointerEnter={onRevealChrome}>
        <span className="chrome-title">{copy.title}</span>
        <div className="chrome-actions">
          <ActionButton id="chrome.backlog" label={copy.history} disabled={!actionEnabled(view, "chrome.backlog")} onAction={onAction} className="chrome-button" />
          <ActionButton id="chrome.menu" label={copy.menu} disabled={!actionEnabled(view, "chrome.menu")} onAction={onAction} className="chrome-button" />
        </div>
      </header>
      {view.choices.length > 0 && (
        <nav className="choice-rail" aria-label={copy.choices}>
          {view.choices.map((choice) => <ChoiceButton key={choice.id} choice={choice} onAction={onAction} wide />)}
        </nav>
      )}
      <section className="reading-band" aria-label={copy.reading}
        aria-keyshortcuts="Enter Space"
        data-page-id={dialogue?.page_id ?? ""}
        data-page-number={dialogue?.page_number ?? 0}
        data-page-count={dialogue?.page_count ?? 0}
        style={{ "--subtitle-columns": dialogue?.columns ?? 80 } as React.CSSProperties}>
        <div className="subtitle-content">
          {dialogue?.speaker && <span className="subtitle-speaker">{dialogue.speaker}</span>}
          <span className="dialogue-text" aria-live="off">{dialogue?.text || ""}</span>
          <span className="sr-only" aria-live="polite" aria-atomic="true">{completedAnnouncement}</span>
        </div>
        <Button className="reading-advance" data-aria-focusable data-aria-action="dialogue.advance" aria-label={copy.next} onPress={() => onAction("dialogue.advance")}>
          {dialogue?.complete && <span className="continue-mark" aria-hidden="true">·</span>}
          <span className="sr-only">{copy.next}</span>
        </Button>
        {modeMark && <span className="reading-mode-mark" aria-label={modeMark}>{modeMark}</span>}
      </section>
    </>
  );
}

type DayCardContent = {
  choice: ChoiceView;
  day: string;
  date: string;
  synopsis: string;
};

function dayCardFor(view: UiViewModel): DayCardContent | null {
  const choice = view.choices[0];
  if (!choice) return null;
  const [day = "", date = "", ...synopsis] = choice.label.split("\n");
  if (!day.trim() || !date.trim() || synopsis.length === 0) return null;
  return {
    choice,
    day: day.trim(),
    date: date.trim(),
    synopsis: synopsis.join("\n").trim(),
  };
}

/**
 * A chapter does not begin with another subtitle.  It pauses on the day's
 * place and weather, gives away only enough to invite the reader onward, and
 * then uses its single semantic choice to enter the prose.  There is no
 * timer: the silence belongs to the player.
 */
function DayCard({ view, copy, onAction }: {
  view: UiViewModel;
  copy: ReturnType<typeof strings>;
  onAction: (id: string) => void;
}) {
  const card = dayCardFor(view);
  if (!card) return null;
  const theme = dayCardThemeFor(view);
  return (
    <section className={`day-card day-card--${theme}`} aria-labelledby="day-card-title">
      <div className="day-card-copy">
        <p className="day-card-date">{card.date}</p>
        <h1 id="day-card-title">{card.day}</h1>
        <p className="day-card-synopsis">{card.synopsis}</p>
      </div>
      <Button
        autoFocus
        className="day-card-advance"
        data-aria-focusable
        data-aria-action={card.choice.id}
        aria-label={copy.next}
        onPress={() => onAction(card.choice.id)}
      >
        <span aria-hidden="true">BEGIN</span>
        <span className="sr-only">{copy.next}</span>
      </Button>
    </section>
  );
}

/**
 * A short, story-owned silence between scenes.  It intentionally does not
 * reuse the subtitle band: Core has already completed and logged its line,
 * while this surface gives that line a full, dark frame and lets any ordinary
 * advance input release the authored hold.
 */
function Interlude({ view, copy, onAction }: {
  view: UiViewModel;
  copy: ReturnType<typeof strings>;
  onAction: (id: string) => void;
}) {
  const text = view.dialogue?.full_page_text || "";
  const firstVisit = view.interlude?.first_visit ?? false;
  return (
    <section
      className={`interlude-screen${firstVisit ? " interlude-screen--first" : " interlude-screen--return"}`}
      aria-label={copy.reading}
    >
      <Button
        autoFocus
        className="interlude-advance"
        data-aria-focusable
        data-aria-action="interlude.advance"
        aria-label={copy.next}
        onPress={() => onAction("interlude.advance")}
      >
        <p className="interlude-line" aria-live="polite" aria-atomic="true">{text}</p>
        <span className="sr-only">{copy.next}</span>
      </Button>
    </section>
  );
}

/**
 * The demo closes as a place in the record, not a sales modal. Its only
 * exits remain inside the installed story: replay the available arc or go
 * back to the title. Store and social links belong to a configured release,
 * never to invented placeholder URLs.
 */
function DemoEnd({ view, copy, onAction }: {
  view: UiViewModel;
  copy: ReturnType<typeof strings>;
  onAction: (id: string) => void;
}) {
  const items: FocusMenuItem[] = view.choices.map((choice, index) => ({
    id: choice.id,
    label: index === 0 ? "READ AGAIN" : "TITLE",
    description: index === 0 ? copy.demoReplay : copy.demoReturn,
    accessibleLabel: choice.label,
  }));
  return (
    <section className="demo-end-screen" aria-labelledby="demo-end-title">
      <div className="demo-end-copy">
        <p className="demo-end-kicker">DEMO COMPLETE</p>
        <h1 id="demo-end-title">{copy.demoComplete}</h1>
        <p className="demo-end-lead">{copy.demoLead}</p>
      </div>
      <FocusMenu
        label={copy.demoComplete}
        items={items}
        onAction={onAction}
        className="demo-end-command-list"
        initialFocusId={items[0]?.id}
        descriptionPlacement="under-focused-item"
      />
    </section>
  );
}

function Title({ view, copy, onAction }: {
  view: UiViewModel;
  copy: ReturnType<typeof strings>;
  onAction: (id: string) => void;
}) {
  const begin = view.choices[0];
  const items: FocusMenuItem[] = [
    ...(begin ? [{ id: begin.id, label: "START", description: copy.menuDescription.start }] : []),
    { id: "route:load", label: "LOAD", description: copy.menuDescription.load, disabled: !actionEnabled(view, "route:load") },
    { id: "route:gallery", label: "EXTRA", description: copy.menuDescription.extra, disabled: !actionEnabled(view, "route:gallery") },
    { id: "route:settings", label: "CONFIG", description: copy.menuDescription.config, disabled: !actionEnabled(view, "route:settings") },
    // The VM already owns this stable action for RMenu. It intentionally
    // remains the same action here so no title-only API is needed.
    { id: "menu.quit", label: "EXIT", description: copy.menuDescription.exit },
  ];
  return (
    <section className="record-title-screen record-title-screen--home" aria-label={copy.title}>
      <StageBackdrop kind="title" />
      <header className="title-identity">
        <div className="title-masthead">
          <h1>{copy.title}</h1>
          <p className="title-subtitle">{copy.subtitle}</p>
          {isDemoEdition && <p className="title-edition">DEMO</p>}
        </div>
      </header>
      <div className="title-selection title-selection--home">
        <FocusMenu
          label={copy.title}
          items={items}
          onAction={onAction}
          className="title-command-list"
          initialFocusId={begin?.id}
          descriptionPlacement="under-focused-item"
        />
      </div>
    </section>
  );
}

async function reopenPresentation() {
  // A player can safely recover from a partially updated PWA shell without
  // touching their IndexedDB saves.  The service worker only owns disposable
  // presentation assets; save data is deliberately outside this cleanup.
  try {
    if ("serviceWorker" in navigator) {
      const scope = new URL("./", document.baseURI).href;
      const registrations = await navigator.serviceWorker.getRegistrations();
      await Promise.all(registrations
        .filter((registration) => registration.scope === scope)
        .map((registration) => registration.unregister()));
    }
    if ("caches" in window) {
      const keys = await caches.keys();
      await Promise.all(keys
        .filter((key) => key.startsWith("umikaze-shell"))
        .map((key) => caches.delete(key)));
    }
  } finally {
    window.location.reload();
  }
}

function RuntimeProblem({ copy, detail }: { copy: ReturnType<typeof strings>; detail: string }) {
  return (
    <section className="runtime-problem" aria-labelledby="runtime-problem-title" role="alert">
      <p className="runtime-problem-kicker">{copy.records}</p>
      <h1 id="runtime-problem-title">{copy.startupIssue}</h1>
      <Button className="runtime-retry" data-aria-focusable onPress={() => { void reopenPresentation(); }}>
        {copy.reopenRecord}
      </Button>
      <p className="runtime-problem-detail">{detail}</p>
    </section>
  );
}

/**
 * The first frame is normally silent.  Only a real slow boot earns a small
 * record-opening card after 250 ms; there is no fake percentage, spinner, or
 * continuous animation competing with the title screen.
 */
function OpeningRecord({ copy }: { copy: ReturnType<typeof strings> }) {
  return (
    <section className="opening-record" aria-live="polite" aria-label={copy.openingRecord}>
      <span>{copy.openingRecord}</span>
    </section>
  );
}

function Setup({ view, copy, onAction }: {
  view: UiViewModel;
  copy: ReturnType<typeof strings>;
  onAction: (id: string) => void;
}) {
  const items: FocusMenuItem[] = view.choices.map((choice, index) => {
    const known = languageNames[index];
    return {
      id: choice.id,
      label: (known?.sublabel || choice.label).toUpperCase(),
      description: copy.languagePrompt,
      accessibleLabel: choice.label,
    };
  });
  return (
    <section className="record-title-screen record-setup-screen" aria-label={copy.firstLight}>
      <StageBackdrop kind="setup" />
      <header className="title-identity">
        <div className="title-masthead">
          <h1>{copy.title}</h1>
          <p className="title-subtitle">{copy.subtitle}</p>
        </div>
      </header>
      <div className="title-selection title-selection--setup">
        <p className="title-selection-kicker">{copy.firstLight}</p>
        <FocusMenu
          label={copy.languagePrompt}
          items={items}
          onAction={onAction}
          className="title-command-list setup-command-list"
          initialFocusId={items[0]?.id}
          descriptionPlacement="under-focused-item"
        />
      </div>
    </section>
  );
}

function Screen({ view, dispatch, chromeVisible, onRevealChrome, saveSlots }: {
  view: UiViewModel;
  dispatch: Dispatch;
  chromeVisible: boolean;
  onRevealChrome: () => void;
  saveSlots: SaveSlotSummary[];
}) {
  const copy = strings(view.game.locale);
  const route = routeName(view.route);
  const onAction = (id: string) => dispatch({ kind: "activate", id });
  if (route === "setup") return <Setup view={view} copy={copy} onAction={onAction} />;
  if (route === "title") return <Title view={view} copy={copy} onAction={onAction} />;
  if (route === "interlude") return <Interlude view={view} copy={copy} onAction={onAction} />;
  if (route === "demo_end") return <DemoEnd view={view} copy={copy} onAction={onAction} />;
  if (route === "day_card") return <DayCard view={view} copy={copy} onAction={onAction} />;
  if (route === "pause") return <RMenu view={view} copy={copy} onAction={onAction} dispatch={dispatch} />;
  if (route === "save" || route === "load") return <SaveLoadSheet kind={route} view={view} copy={copy} onAction={onAction} dispatch={dispatch} saveSlots={saveSlots} />;
  if (route === "settings") return <SettingsSheet view={view} copy={copy} dispatch={dispatch} />;
  if (route === "backlog") return <BacklogSheet view={view} copy={copy} onAction={onAction} dispatch={dispatch} />;
  // The invitation to choose a chapter is story text, not a modal. Let it
  // occupy the same quiet reading surface as the novel before the catalogue
  // itself arrives on the following advance.
  if (route === "chapter_select" && view.dialogue && view.choices.length === 0) {
    return <Dialogue view={view} copy={copy} onAction={onAction} chromeVisible={chromeVisible} onRevealChrome={onRevealChrome} />;
  }
  if (route === "chapter_select") return <ChapterSheet view={view} copy={copy} onAction={onAction} dispatch={dispatch} />;
  if (route === "gallery") return <GallerySheet view={view} copy={copy} onAction={onAction} dispatch={dispatch} />;
  if (route === "confirm") return <ConfirmationSheet view={view} copy={copy} onAction={onAction} dispatch={dispatch} />;
  return <Dialogue view={view} copy={copy} onAction={onAction} chromeVisible={chromeVisible} onRevealChrome={onRevealChrome} />;
}

export default function App() {
  const canvas = useRef<HTMLCanvasElement>(null);
  const runtime = useRef<PresentationRuntime | null>(null);
  const focusBeforeOverlay = useRef<HTMLElement | null>(null);
  const focusRestoreAction = useRef<string | null>(null);
  const lastFocusable = useRef<HTMLElement | null>(null);
  const previousWasOverlay = useRef(false);
  const chromeTimer = useRef<number | null>(null);
  const [output, setOutput] = useState<AriaStepOutput | null>(null);
  const [status, setStatus] = useState("");
  const [bootSlow, setBootSlow] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [chromeVisible, setChromeVisible] = useState(false);
  const [saveSlots, setSaveSlots] = useState<SaveSlotSummary[]>([]);

  useEffect(() => {
    const target = canvas.current;
    if (!target) return;
    let alive = true;
    const slowBootTimer = window.setTimeout(() => {
      if (alive) setBootSlow(true);
    }, 250);
    void bootPresentation(target, {
      onOutput(next) {
        if (!alive) return;
        window.clearTimeout(slowBootTimer);
        setBootSlow(false);
        setOutput(next);
      },
      onStatus(message) { if (alive) setStatus(message); },
      onError(cause) {
        if (!alive) return;
        window.clearTimeout(slowBootTimer);
        setBootSlow(false);
        setError(cause.message);
      },
      onSaveSlots(next) { if (alive) setSaveSlots(next); },
    }).then((controller) => {
      if (alive) runtime.current = controller;
      else controller.dispose();
    }).catch((cause: unknown) => {
      if (!alive) return;
      window.clearTimeout(slowBootTimer);
      setBootSlow(false);
      setError(cause instanceof Error ? cause.message : String(cause));
    });
    return () => {
      alive = false;
      window.clearTimeout(slowBootTimer);
      runtime.current?.dispose();
      runtime.current = null;
    };
  }, []);

  const view = output?.view;
  const fallbackCopy = strings(view?.game.locale ?? navigator.language);
  const route = view ? routeName(view.route) : "loading";
  const isInterlude = route === "interlude";
  const tone = toneForScene(output);
  const dayCardTheme = dayCardThemeFor(view);
  const isReadingStage = route === "dialogue"
    || (route === "chapter_select" && Boolean(view?.dialogue) && (view?.choices.length ?? 0) === 0);
  const sceneDirection = isReadingStage
    ? directionForScene(output, Boolean(view?.settings.reduced_motion))
    : emptySceneDirection;

  useEffect(() => () => {
    if (chromeTimer.current !== null) window.clearTimeout(chromeTimer.current);
  }, []);

  useEffect(() => {
    if (route !== "dialogue" && route !== "chapter_select") {
      if (chromeTimer.current !== null) window.clearTimeout(chromeTimer.current);
      chromeTimer.current = null;
      setChromeVisible(false);
    }
  }, [route]);

  const revealChrome = () => {
    if (route !== "dialogue" && route !== "chapter_select") return;
    setChromeVisible(true);
    if (chromeTimer.current !== null) window.clearTimeout(chromeTimer.current);
    chromeTimer.current = window.setTimeout(() => {
      setChromeVisible(false);
      chromeTimer.current = null;
    }, 1800);
  };
  useEffect(() => {
    if (!view) return;
    document.documentElement.lang = localeFor(view.game.locale);
    if (view.settings.fullscreen && !document.fullscreenElement) {
      void document.documentElement.requestFullscreen?.().catch(() => {});
    }
    if (!view.settings.fullscreen && document.fullscreenElement) {
      void document.exitFullscreen?.().catch(() => {});
    }
  }, [view?.game.locale, view?.settings.fullscreen]);

  // React Aria presents sheets in a document-level overlay container. Mirror
  // the two visual accessibility states onto <html> so those non-reading
  // layers receive the same treatment without widening any reading selector.
  useEffect(() => {
    const root = document.documentElement;
    root.classList.toggle("umikaze-high-contrast", Boolean(view?.settings.high_contrast));
    root.classList.toggle("umikaze-reduced-motion", Boolean(view?.settings.reduced_motion));
    return () => {
      root.classList.remove("umikaze-high-contrast", "umikaze-reduced-motion");
    };
  }, [view?.settings.high_contrast, view?.settings.reduced_motion]);

  useEffect(() => {
    const wasOverlay = previousWasOverlay.current;
    const nowOverlay = isOverlayRoute(route, view);
    if (!wasOverlay && nowOverlay) {
      // React Aria moves focus into its modal during the commit. Retain the
      // last focused control from the presentation layer rather than sampling
      // `activeElement` after that move, so Escape returns to the opener.
      focusBeforeOverlay.current = lastFocusable.current
        ?? (document.activeElement instanceof HTMLElement ? document.activeElement : null);
      focusRestoreAction.current ??= focusBeforeOverlay.current?.getAttribute("data-aria-action") ?? null;
    }
    if (wasOverlay && !nowOverlay) {
      const target = focusBeforeOverlay.current;
      const action = focusRestoreAction.current;
      window.requestAnimationFrame(() => {
        // React Aria restores the focus scope during its own first frame.
        // Run immediately afterwards and only when a new sheet has not
        // opened, preserving the originating control for keyboard users.
        window.requestAnimationFrame(() => {
          if (document.querySelector('[role="dialog"]')) return;
          const recreatedTarget = action
            ? [...document.querySelectorAll<HTMLElement>("[data-aria-action]")]
              .find((element) => element.getAttribute("data-aria-action") === action)
            : null;
          if (recreatedTarget && !recreatedTarget.matches(":disabled")) {
            recreatedTarget.focus({ preventScroll: true });
            return;
          }
          if (target?.isConnected && !target.matches(":disabled")) {
            target.focus({ preventScroll: true });
            return;
          }
          document.querySelector<HTMLElement>("[data-aria-focusable]")?.focus({ preventScroll: true });
        });
      });
      focusBeforeOverlay.current = null;
      focusRestoreAction.current = null;
    }
    previousWasOverlay.current = nowOverlay;
  }, [route, view]);

  const dispatch: Dispatch = (intent) => {
    if (
      intent.kind === "activate"
      && view
      && !isOverlayRoute(route, view)
      && intent.id !== "dialogue.advance"
      && intent.id !== "interlude.advance"
    ) {
      const active = document.activeElement;
      if (active instanceof HTMLElement && active.matches("[data-aria-focusable]")) {
        focusBeforeOverlay.current = active;
        focusRestoreAction.current = active.getAttribute("data-aria-action");
      }
    }
    runtime.current?.intent(intent);
  };
  const rememberFocusable = (target: EventTarget | null) => {
    const element = target instanceof Element ? target : null;
    const control = element?.closest<HTMLElement>("[data-aria-focusable]");
    if (control) lastFocusable.current = control;
  };
  const openRMenu = (event: React.MouseEvent<HTMLElement>) => {
    const target = event.target instanceof Element ? event.target : null;
    if (target?.closest("input, textarea, select, [contenteditable=true]")) return;
    // This is an installed game surface, not a browser document. Suppress the
    // WebView's context menu on every stage route; where a semantic layer is
    // available, keep the familiar right-click meaning instead.
    event.preventDefault();
    if (!isOverlayRoute(route, view) && view && actionEnabled(view, "chrome.menu")) {
      dispatch({ kind: "activate", id: "chrome.menu" });
    } else if (isOverlayRoute(route, view)) {
      // A secondary click closes only the foremost semantic layer.  In
      // particular, it returns from a CG viewer to its grid instead of
      // discarding every sheet underneath it.
      dispatch({ kind: "dismiss" });
    }
  };
  const capturePointer = (event: React.PointerEvent<HTMLElement>) => {
    if (event.button === 2) {
      // React Aria's press abstraction must never treat a secondary click as
      // a reading advance. The following contextmenu event opens rmenu.
      event.preventDefault();
      event.stopPropagation();
      return;
    }
    rememberFocusable(event.target);
  };
  const observePointer = (event: React.PointerEvent<HTMLElement>) => {
    // The two quiet controls belong to the top edge rather than the reading
    // surface. This keeps the scene clear, while remaining discoverable with
    // a mouse or pen and fully reachable from the keyboard.
    if (event.clientY <= 84) revealChrome();
  };
  const wheelGesture = useRef({ accumulated: 0, consumed: false, lastAt: 0 });
  const readingAdvanceAction = () => {
    if (!view) return null;
    if (route === "day_card") return view.choices[0]?.id ?? null;
    if (route === "interlude") return "interlude.advance";
    if (
      (route === "dialogue" || (route === "chapter_select" && Boolean(view.dialogue)))
      && view.choices.length === 0
    ) {
      return "dialogue.advance";
    }
    return null;
  };
  const useReadingEdge = (event: React.MouseEvent<HTMLElement>) => {
    // A visual novel cannot leave arbitrary parts of the frame inert. Any
    // ordinary primary click on a reading surface has the same meaning as the
    // explicit Next button. The compact chrome and every semantic control
    // remain independently interactive, so a player can still open LOG or
    // RMenu without accidentally advancing.
    if (!view || event.detail === 0 || event.button !== 0 || isInteractiveTarget(event.target)) return;
    const advanceAction = readingAdvanceAction();
    if (advanceAction) {
      event.preventDefault();
      event.stopPropagation();
      dispatch({ kind: "activate", id: advanceAction });
    }
  };
  const useReadingWheel = (event: React.WheelEvent<HTMLElement>) => {
    if (event.deltaY <= 0 || isInteractiveTarget(event.target)) return;
    const advanceAction = readingAdvanceAction();
    if (!advanceAction) return;

    // A wheel notch or one deliberate trackpad gesture advances exactly one
    // unit. A sustained fling must not race through several subtitle pages.
    event.preventDefault();
    event.stopPropagation();
    const now = performance.now();
    if (now - wheelGesture.current.lastAt > 180) {
      wheelGesture.current.accumulated = 0;
      wheelGesture.current.consumed = false;
    }
    wheelGesture.current.lastAt = now;
    if (wheelGesture.current.consumed) return;
    wheelGesture.current.accumulated += Math.abs(event.deltaY);
    const threshold = event.deltaMode === WheelEvent.DOM_DELTA_PIXEL ? 40 : 1;
    if (wheelGesture.current.accumulated < threshold) return;
    wheelGesture.current.accumulated = 0;
    wheelGesture.current.consumed = true;
    dispatch({ kind: "activate", id: advanceAction });
  };
  return (
    <main
      className={`umikaze route-${route} scene-tone-${tone}${route === "day_card" ? ` day-card-theme-${dayCardTheme}` : ""}${view?.choices.length ? " has-choices" : ""}${view?.settings.high_contrast ? " high-contrast" : ""}${view?.settings.reduced_motion ? " reduce-motion" : ""}${view?.settings.stage_effects === false ? " stage-effects-off" : ""}`}
      style={{
        "--text-scale": view?.settings.text_scale ?? 1,
        "--subtitle-opacity": view?.settings.text_opacity ?? 1,
      } as React.CSSProperties}
      onContextMenuCapture={openRMenu}
      onPointerDownCapture={capturePointer}
      onPointerMoveCapture={observePointer}
      onClickCapture={useReadingEdge}
      onWheelCapture={useReadingWheel}
      onFocusCapture={(event) => {
        rememberFocusable(event.target);
        if (event.target instanceof Element && event.target.closest(".quiet-chrome")) revealChrome();
      }}
    >
      <canvas ref={canvas} className="scene-canvas" data-aria-stage="dom" aria-hidden="true" />
      {view && !isInterlude && <>
        <ScenePhotograph output={output} transform={sceneDirection.transform} />
        <div className="atmosphere" style={sceneDirection.transform ? { transform: sceneDirection.transform } : undefined} aria-hidden="true" />
        <SceneDirectionLayer overlays={sceneDirection.overlays} />
      </>}
      <div className="presentation-layer">
        {view && <Screen view={view} dispatch={dispatch} chromeVisible={chromeVisible} onRevealChrome={revealChrome} saveSlots={saveSlots} />}
        {!view && !error && bootSlow && <OpeningRecord copy={fallbackCopy} />}
        {error && <RuntimeProblem copy={fallbackCopy} detail={error} />}
      </div>
      {!error && view && status && <p className="runtime-status" role="status" aria-live="polite">{status}</p>}
    </main>
  );
}
