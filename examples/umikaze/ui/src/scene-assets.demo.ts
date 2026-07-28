import coastRoad from "./assets/scenes/coast-road-dawn-v1.webp";
import hospitalCorridor from "./assets/scenes/hospital-corridor-overcast-v1.webp";
import rainWindow from "./assets/scenes/rain-window-dusk-v1.webp";
import trainWindowSummer from "./assets/scenes/train-window-summer-v1.webp";
import trainMotionSummer from "./assets/scenes/train-motion-summer-v1.webp";
import stationNightPass from "./assets/scenes/station-night-pass-v1.webp";
import railWindowSunset from "./assets/scenes/rail-window-sunset-v1.webp";
import shoreStormSunset from "./assets/scenes/shore-storm-sunset-v1.webp";
import platformSeaDawn from "./assets/scenes/platform-sea-dawn-v1.webp";
import hotelCorridorBlue from "./assets/scenes/hotel-corridor-blue-v1.webp";
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
  hotel: { source: sceneSources.ward, name: "corridor" },
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
  blackout: { name: "blackout", solid: "#05070b" },
  whiteout: { name: "whiteout", solid: "#ded7c9" },
  stillness: { name: "stillness", solid: "#6d706f" },
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
