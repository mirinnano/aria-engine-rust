// A closed, opening-arc-only stage vocabulary. This is intentionally separate
// from the full map so a public demo bundle cannot retain later chapter names
// merely because the shared presentation knows how to render them.
export const sceneToneByColor: Record<string, string> = {
  "16,43,56": "tide",
  "40,75,89": "school",
  "61,70,85": "ward",
  "36,78,90": "blue",
  "57,72,87": "rain",
  "5,7,11": "blackout",
  "15,47,57": "city",
  "118,110,97": "rail-sunset",
  "79,75,83": "hotel-blue",
  "80,100,115": "north-platform",
  "109,112,111": "stillness",
};

// Day cards have their own environmental reading. A chapter opens on the
// weather of that day rather than inheriting a generic menu photograph.
export const dayCardToneByHeading: Record<string, string> = {
  PROLOGUE: "ward",
  "DAY 1": "north-platform",
  "DAY 2": "hotel-blue",
  "DAY 3": "hotel-blue",
  "DAY 4": "blue",
};

export const dayCardThemeByHeading: Record<string, string> = {
  PROLOGUE: "ward",
  "DAY 1": "departure",
  "DAY 2": "rain",
  "DAY 3": "rail",
  "DAY 4": "shore",
};
