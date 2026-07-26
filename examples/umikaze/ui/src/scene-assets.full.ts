import coastRoad from "./assets/scenes/coast-road-dawn-v1.webp";
import hospitalCorridor from "./assets/scenes/hospital-corridor-overcast-v1.webp";
import rainWindow from "./assets/scenes/rain-window-dusk-v1.webp";
import trainWindowSummer from "./assets/scenes/train-window-summer-v1.webp";
import trainMotionSummer from "./assets/scenes/train-motion-summer-v1.webp";
import understructureEvening from "./assets/scenes/understructure-evening-v1.webp";
import stationNightPass from "./assets/scenes/station-night-pass-v1.webp";
import nightWindowMotion from "./assets/scenes/night-window-motion-v1.webp";
import blueTwilight from "./assets/scenes/blue-twilight-v1.webp";
import railPlatformDawn from "./assets/scenes/rail-platform-dawn-v1.webp";
import mistWindowRail from "./assets/scenes/mist-window-rail-v1.webp";
import railWindowSunset from "./assets/scenes/rail-window-sunset-v1.webp";
import shoreStormSunset from "./assets/scenes/shore-storm-sunset-v1.webp";
import neonAlley from "./assets/scenes/neon-alley-v1.webp";
import rainStreetEvening from "./assets/scenes/rain-street-evening-v1.webp";
import bridgeUnderstructure from "./assets/scenes/bridge-understructure-v1.webp";
import passageSunset from "./assets/scenes/passage-sunset-v1.webp";
import type { SceneAsset, StagePhoto } from "./scene-assets.types";

export const sceneSources: Record<string, string> = {
  coast: coastRoad,
  ward: hospitalCorridor,
  rain: rainWindow,
  school: trainWindowSummer,
  setup: trainMotionSummer,
  understructure: understructureEvening,
  station: stationNightPass,
  night: nightWindowMotion,
  blue: blueTwilight,
  platform: railPlatformDawn,
  mist: mistWindowRail,
  "rail-sunset": railWindowSunset,
  shore: shoreStormSunset,
  city: neonAlley,
  "rain-city": rainStreetEvening,
  bridge: bridgeUnderstructure,
  passage: passageSunset,
};

export const sceneAssetByTone: Record<string, SceneAsset> = {
  loading: { source: sceneSources.coast, name: "coast" },
  title: { source: sceneSources.coast, name: "coast" },
  coast: { source: sceneSources.coast, name: "coast" },
  tide: { source: sceneSources.coast, name: "coast" },
  ward: { source: sceneSources.ward, name: "corridor" },
  school: { source: sceneSources.school, name: "summer-window" },
  station: { source: sceneSources.station, name: "station" },
  motion: { source: sceneSources.night, name: "night-motion" },
  platform: { source: sceneSources.platform, name: "platform" },
  mist: { source: sceneSources.mist, name: "mist-rail" },
  "rail-sunset": { source: sceneSources["rail-sunset"], name: "rail-sunset" },
  hotel: { source: sceneSources.ward, name: "corridor" },
  blue: { source: sceneSources.blue, name: "blue-twilight" },
  city: { source: sceneSources.city, name: "neon" },
  "rain-city": { source: sceneSources["rain-city"], name: "rain-street" },
  bridge: { source: sceneSources.bridge, name: "bridge" },
  passage: { source: sceneSources.passage, name: "passage" },
  shore: { source: sceneSources.shore, name: "storm-shore" },
  rain: { source: sceneSources.rain, name: "rain" },
  night: { source: sceneSources.night, name: "night-motion" },
  clear: { source: sceneSources.school, name: "summer-window" },
  harbor: { source: sceneSources.coast, name: "coast" },
  blackout: { name: "blackout", solid: "#05070b" },
  whiteout: { name: "whiteout", solid: "#ded7c9" },
};

export const stagePhotoByKind: Record<string, StagePhoto> = {
  title: { source: sceneSources.school, name: "window" },
  setup: { source: sceneSources.setup, name: "motion" },
  record: { source: sceneSources.understructure, name: "understructure" },
};

export const chapterFallbackSources = [sceneSources.coast, sceneSources.ward, sceneSources.rain] as const;
export const gallerySources = [sceneSources.coast, sceneSources.ward, sceneSources.rain] as const;
