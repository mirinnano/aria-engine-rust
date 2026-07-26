/**
 * Layout-free TypeScript contract for an Aria game presentation.
 *
 * This package deliberately contains no theme, pixel geometry, DOM IDs, or
 * game copy.  It is the narrow seam that lets a game-owned React frontend
 * consume the deterministic WASM VM without recreating a renderer DSL.
 */

export const UI_VIEW_MODEL_SCHEMA = 6 as const;

export type StandardRoute =
  | "setup"
  | "title"
  | "demo_end"
  | "dialogue"
  | "pause"
  | "save"
  | "load"
  | "settings"
  | "backlog"
  | "chapter_select"
  | "gallery"
  | "confirm";

export type UiRoute =
  | { kind: StandardRoute }
  | { kind: "custom"; name: string };

export type UiIntent =
  | { kind: "activate"; id: string }
  | { kind: "open_route"; route: string }
  | { kind: "dismiss" }
  | { kind: "set_setting"; name: string; value: number }
  | { kind: "toggle_setting"; name: string }
  | { kind: "scroll"; region: string; delta_y: number };

export interface SettingsView {
  text_speed_ms: number;
  auto_delay_ms: number;
  bgm_volume: number;
  sound_effect_volume: number;
  voice_volume: number;
  fullscreen: boolean;
  text_scale: number;
  text_opacity: number;
  high_contrast: boolean;
  reduced_motion: boolean;
  stage_effects: boolean;
  skip_unread: boolean;
}

export interface DialogueView {
  speaker: string | null;
  full_text: string;
  full_page_text: string;
  page_number: number;
  page_count: number;
  page_id: string;
  columns: number;
  text: string;
  complete: boolean;
  awaiting_advance: boolean;
}

export interface ChoiceView {
  id: string;
  label: string;
  selected: boolean;
}

export interface ActionView {
  id: string;
  enabled: boolean;
  active: boolean;
}

export interface BacklogEntryView {
  id: string;
  resume_id: string;
  speaker: string | null;
  text: string;
  locale: string;
  timestamp_ms: number;
  selected: boolean;
}

export interface ChapterView {
  id: string;
  title: string;
  description: string;
  thumbnail: string | null;
  script: string | null;
  unlocked: boolean;
  progress: number;
}

export interface GalleryItemView {
  id: string;
  unlocked: boolean;
  selected: boolean;
}

export interface InterludeView {
  first_visit: boolean;
}

export interface ConfirmationView {
  action: "reset" | "quit" | string;
  resume_id: string | null;
}

export interface UiViewModel {
  schema_version: number;
  route: UiRoute;
  route_stack: UiRoute[];
  game: { id: string; locale: string };
  dialogue: DialogueView | null;
  choices: ChoiceView[];
  actions: ActionView[];
  settings: SettingsView;
  backlog: BacklogEntryView[];
  backlog_total: number;
  backlog_start: number;
  chapters: ChapterView[];
  gallery: GalleryItemView[];
  gallery_viewer: string | null;
  interlude: InterludeView | null;
  confirmation: ConfirmationView | null;
  scroll_offsets: Record<string, number>;
  auto_mode: "off" | "on";
  skip_mode: "off" | "read" | "all";
  reduced_motion: boolean;
}

export interface SceneFrame {
  frame_number: number;
  logical_size: { width: number; height: number };
  // Scene commands are intentionally opaque to the React UI. They go
  // straight to the shared scene renderer.
  commands: unknown[];
}

export interface RuntimeCommand {
  kind: string;
  slot?: number;
}

export interface AriaStepOutput {
  scene: SceneFrame;
  view: UiViewModel;
  audio: unknown[];
  runtime: RuntimeCommand[];
  halted: boolean;
}

export function routeName(route: UiRoute): string {
  return route.kind === "custom" ? route.name : route.kind;
}

export function actionEnabled(view: UiViewModel, id: string): boolean {
  return view.actions.find((action) => action.id === id)?.enabled ?? false;
}

/** Throws early when a frontend/runtime pair does not share a contract. */
export function assertViewModel(value: unknown): asserts value is UiViewModel {
  if (!value || typeof value !== "object") {
    throw new Error("Aria presentation runtime returned no UI view model");
  }
  const model = value as Partial<UiViewModel>;
  if (model.schema_version !== UI_VIEW_MODEL_SCHEMA) {
    throw new Error(
      `Unsupported Aria UI view model schema ${String(model.schema_version)}; expected ${UI_VIEW_MODEL_SCHEMA}`,
    );
  }
  if (
    !model.route
    || !model.game
    || !model.settings
    || !Array.isArray(model.actions)
    || !("interlude" in model)
  ) {
    throw new Error("Aria presentation runtime returned an incomplete UI view model");
  }
}
