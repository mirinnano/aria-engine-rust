export type SceneAsset = {
  source?: string;
  name: string;
  solid?: string;
};

export type StagePhoto = {
  source: string;
  name: "window" | "motion" | "night-motion" | "understructure";
};
