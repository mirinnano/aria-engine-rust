import coastRoad from "../../assets/bg/scenes/coast-road-dawn-v1.webp";
import hospitalCorridor from "../../assets/bg/scenes/hospital-corridor-overcast-v1.webp";
import rainWindow from "../../assets/bg/scenes/rain-window-dusk-v1.webp";
import trainWindowSummer from "../../assets/bg/scenes/train-window-summer-v1.webp";
import trainMotionSummer from "../../assets/bg/scenes/train-motion-summer-v1.webp";
import understructureEvening from "../../assets/bg/scenes/understructure-evening-v1.webp";
import stationNightPass from "../../assets/bg/scenes/station-night-pass-v1.webp";
import nightWindowMotion from "../../assets/bg/scenes/night-window-motion-v1.webp";
import blueTwilight from "../../assets/bg/scenes/blue-twilight-v1.webp";
import railPlatformDawn from "../../assets/bg/scenes/rail-platform-dawn-v1.webp";
import mistWindowRail from "../../assets/bg/scenes/mist-window-rail-v1.webp";
import railWindowSunset from "../../assets/bg/scenes/rail-window-sunset-v1.webp";
import shoreStormSunset from "../../assets/bg/scenes/shore-storm-sunset-v1.webp";
import neonAlley from "../../assets/bg/scenes/neon-alley-v1.webp";
import rainStreetEvening from "../../assets/bg/scenes/rain-street-evening-v1.webp";
import bridgeUnderstructure from "../../assets/bg/scenes/bridge-understructure-v1.webp";
import passageSunset from "../../assets/bg/scenes/passage-sunset-v1.webp";
import platformSeaDawn from "../../assets/bg/scenes/platform-sea-dawn-v1.webp";
import ferryFogMorning from "../../assets/bg/scenes/ferry-fog-morning-v1.webp";
import hotelCorridorBlue from "../../assets/bg/scenes/hotel-corridor-blue-v1.webp";
import trainRainGrey from "../../assets/bg/scenes/train-rain-grey-v1.webp";
import sannomiyaRainPlatform from "../../assets/bg/scenes/sannomiya-rain-platform-v1.webp";
import okayamaRailWindow from "../../assets/bg/scenes/okayama-rail-window-v1.webp";
import ferryNightDeck from "../../assets/bg/scenes/ferry-night-deck-v1.webp";
import mountainRoomDawn from "../../assets/bg/scenes/mountain-room-dawn-v1.webp";
import terminusGreySea from "../../assets/bg/scenes/terminus-grey-sea-v1.webp";
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
  "north-platform": platformSeaDawn,
  ferry: ferryFogMorning,
  "hotel-blue": hotelCorridorBlue,
  "train-grey": trainRainGrey,
  "sannomiya-rain-platform": sannomiyaRainPlatform,
  "okayama-rail-window": okayamaRailWindow,
  "ferry-night-deck": ferryNightDeck,
  "mountain-room-dawn": mountainRoomDawn,
  "terminus-grey-sea": terminusGreySea,
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
  "north-platform": { source: sceneSources["north-platform"], name: "north-platform" },
  ferry: { source: sceneSources.ferry, name: "ferry-fog" },
  "hotel-blue": { source: sceneSources["hotel-blue"], name: "hotel-blue" },
  "train-grey": { source: sceneSources["train-grey"], name: "train-grey" },
  "sannomiya-rain-platform": { source: sceneSources["sannomiya-rain-platform"], name: "sannomiya-rain-platform" },
  "okayama-rail-window": { source: sceneSources["okayama-rail-window"], name: "okayama-rail-window" },
  "ferry-night-deck": { source: sceneSources["ferry-night-deck"], name: "ferry-night-deck" },
  "mountain-room-dawn": { source: sceneSources["mountain-room-dawn"], name: "mountain-room-dawn" },
  "terminus-grey-sea": { source: sceneSources["terminus-grey-sea"], name: "terminus-grey-sea" },
  blackout: { name: "blackout", solid: "#05070b" },
  whiteout: { name: "whiteout", solid: "#ded7c9" },
  stillness: { name: "stillness", solid: "#6d706f" },
};

/** Story-owned logical background paths.  The same names are used by the
 * Aria PAK and by the Web presentation, so a location never depends on a
 * colour-code heuristic. */
export const sceneAssetByLogicalPath: Record<string, SceneAsset> = {
  "assets/bg/scenes/hospital-corridor-overcast-v1.webp": { source: hospitalCorridor, name: "corridor" },
  "assets/bg/scenes/platform-sea-dawn-v1.webp": { source: platformSeaDawn, name: "platform-sea-dawn" },
  "assets/bg/scenes/sannomiya-rain-platform-v1.webp": { source: sannomiyaRainPlatform, name: "sannomiya-rain-platform" },
  "assets/bg/scenes/okayama-rail-window-v1.webp": { source: okayamaRailWindow, name: "okayama-rail-window" },
  "assets/bg/scenes/shore-storm-sunset-v1.webp": { source: shoreStormSunset, name: "storm-shore" },
  "assets/bg/scenes/rain-street-evening-v1.webp": { source: rainStreetEvening, name: "rain-street" },
  "assets/bg/scenes/coast-road-dawn-v1.webp": { source: coastRoad, name: "coast-road" },
  "assets/bg/scenes/ferry-night-deck-v1.webp": { source: ferryNightDeck, name: "ferry-night-deck" },
  "assets/bg/scenes/mountain-room-dawn-v1.webp": { source: mountainRoomDawn, name: "mountain-room-dawn" },
  "assets/bg/scenes/train-rain-grey-v1.webp": { source: trainRainGrey, name: "train-grey" },
  "assets/bg/scenes/terminus-grey-sea-v1.webp": { source: terminusGreySea, name: "terminus-grey-sea" },
  "assets/bg/scenes/hotel-corridor-blue-v1.webp": { source: hotelCorridorBlue, name: "hotel-blue" },
  "assets/bg/scenes/rail-window-sunset-v1.webp": { source: railWindowSunset, name: "rail-sunset" },
  "assets/bg/scenes/neon-alley-v1.webp": { source: neonAlley, name: "neon" },
};

export const stagePhotoByKind: Record<string, StagePhoto> = {
  title: { source: sceneSources.night, name: "night-motion" },
  setup: { source: sceneSources.setup, name: "motion" },
  record: { source: sceneSources.understructure, name: "understructure" },
};

export const chapterFallbackSources = [sceneSources.coast, sceneSources.ward, sceneSources.rain] as const;
export const gallerySources = [sceneSources.coast, sceneSources.ward, sceneSources.rain] as const;
