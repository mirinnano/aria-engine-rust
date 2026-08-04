import coastRoad from "../../assets/bg/scenes/coast-road-dawn-v1.webp";
import hospitalCorridor from "../../assets/bg/scenes/hospital-corridor-overcast-v1.webp";
import rainWindow from "../../assets/bg/scenes/rain-window-dusk-v1.webp";
import trainWindowSummer from "../../assets/bg/scenes/train-window-summer-v1.webp";
import trainMotionSummer from "../../assets/bg/scenes/train-motion-summer-v1.webp";
import stationNightPass from "../../assets/bg/scenes/station-night-pass-v1.webp";
import railWindowSunset from "../../assets/bg/scenes/rail-window-sunset-v1.webp";
import shoreStormSunset from "../../assets/bg/scenes/shore-storm-sunset-v1.webp";
import platformSeaDawn from "../../assets/bg/scenes/platform-sea-dawn-v1.webp";
import hotelCorridorBlue from "../../assets/bg/scenes/hotel-corridor-blue-v1.webp";
import sannomiyaRainPlatform from "../../assets/bg/scenes/sannomiya-rain-platform-v1.webp";
import okayamaRailWindow from "../../assets/bg/scenes/okayama-rail-window-v1.webp";
import type { SceneAsset, StagePhoto } from "./scene-assets.types";

// The browser demo is deliberately a closed opening arc. It reuses its
// approved opening-arc photographs for neutral system states instead of
// shipping visual material from later chapters merely to decorate an
// unreachable route.
export const sceneSources: Record<string, string> = {
  coast: coastRoad,
  ward: hospitalCorridor,
  rain: rainWindow,
  school: trainWindowSummer,
  setup: trainMotionSummer,
  station: stationNightPass,
  "rail-sunset": railWindowSunset,
  shore: shoreStormSunset,
  "north-platform": platformSeaDawn,
  "hotel-blue": hotelCorridorBlue,
  "sannomiya-rain-platform": sannomiyaRainPlatform,
  "okayama-rail-window": okayamaRailWindow,
};

export const sceneAssetByTone: Record<string, SceneAsset> = {
  loading: { source: sceneSources.coast, name: "coast" },
  title: { source: sceneSources.coast, name: "coast" },
  coast: { source: sceneSources.coast, name: "coast" },
  tide: { source: sceneSources.coast, name: "coast" },
  ward: { source: sceneSources.ward, name: "corridor" },
  school: { source: sceneSources.school, name: "summer-window" },
  station: { source: sceneSources.station, name: "station" },
  motion: { source: sceneSources.setup, name: "train-motion" },
  platform: { source: sceneSources.station, name: "station" },
  mist: { source: sceneSources["rail-sunset"], name: "rail-sunset" },
  "rail-sunset": { source: sceneSources["rail-sunset"], name: "rail-sunset" },
  // A hotel is not a hospital. Keep the fallback semantically honest when
  // a native logical-path command is not available during a transitional
  // frame or while an older save is being restored.
  hotel: { source: sceneSources["hotel-blue"], name: "hotel-blue" },
  blue: { source: sceneSources.shore, name: "storm-shore" },
  city: { source: sceneSources.rain, name: "rain" },
  "rain-city": { source: sceneSources.rain, name: "rain" },
  bridge: { source: sceneSources["rail-sunset"], name: "rail-sunset" },
  passage: { source: sceneSources.coast, name: "coast" },
  shore: { source: sceneSources.shore, name: "storm-shore" },
  rain: { source: sceneSources.rain, name: "rain" },
  night: { source: sceneSources.setup, name: "train-motion" },
  clear: { source: sceneSources.school, name: "summer-window" },
  harbor: { source: sceneSources.coast, name: "coast" },
  "north-platform": { source: sceneSources["north-platform"], name: "north-platform" },
  "hotel-blue": { source: sceneSources["hotel-blue"], name: "hotel-blue" },
  "sannomiya-rain-platform": { source: sceneSources["sannomiya-rain-platform"], name: "sannomiya-rain-platform" },
  "okayama-rail-window": { source: sceneSources["okayama-rail-window"], name: "okayama-rail-window" },
  blackout: { name: "blackout", solid: "#05070b" },
  whiteout: { name: "whiteout", solid: "#ded7c9" },
  stillness: { name: "stillness", solid: "#6d706f" },
};

// Keep the same logical-path seam as the full edition. The demo only exposes
// its opening-arc photographs; later story assets intentionally fall back to
// the neutral tone map instead of leaking unreleased material.
export const sceneAssetByLogicalPath: Record<string, SceneAsset> = {
  "assets/bg/scenes/hospital-corridor-overcast-v1.webp": { source: hospitalCorridor, name: "corridor" },
  "assets/bg/scenes/platform-sea-dawn-v1.webp": { source: platformSeaDawn, name: "platform-sea-dawn" },
  "assets/bg/scenes/hotel-corridor-blue-v1.webp": { source: hotelCorridorBlue, name: "hotel-blue" },
  "assets/bg/scenes/shore-storm-sunset-v1.webp": { source: shoreStormSunset, name: "storm-shore" },
  "assets/bg/scenes/rail-window-sunset-v1.webp": { source: railWindowSunset, name: "rail-sunset" },
  "assets/bg/scenes/coast-road-dawn-v1.webp": { source: coastRoad, name: "coast-road" },
  "assets/bg/scenes/sannomiya-rain-platform-v1.webp": { source: sannomiyaRainPlatform, name: "sannomiya-rain-platform" },
  "assets/bg/scenes/okayama-rail-window-v1.webp": { source: okayamaRailWindow, name: "okayama-rail-window" },
};

export const stagePhotoByKind: Record<string, StagePhoto> = {
  // Station Night Pass preserves the nocturnal title register without
  // leaking the later chapter's night-window photograph into the demo.
  title: { source: sceneSources.station, name: "night-motion" },
  setup: { source: sceneSources.setup, name: "motion" },
  record: { source: sceneSources.ward, name: "understructure" },
};

export const chapterFallbackSources = [sceneSources.coast, sceneSources.ward, sceneSources.rain] as const;
export const gallerySources = [sceneSources.coast, sceneSources.ward, sceneSources.rain] as const;
