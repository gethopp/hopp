// Hand-picked colors for the tailwind colors page:
// https://tailwindcss.com/docs/colors
export const SVG_BADGE_COLORS = [
  "#0040FF",
  "#7CCF00",
  "#615FFF",
  "#009689",
  "#C800DE",
  "#00A6F4",
  "#FFB900",
  "#ED0040",
];

// Singleton Participant color map
const participantColorMap: Map<string, string> = new Map();
// Helper function to get or assign a color for a participant
// Only assigns a color on entry creation (when participant is first encountered)
export const getOrAssignColor = (participantId: string): string => {
  if (participantColorMap.has(participantId)) {
    return participantColorMap.get(participantId)!;
  }
  const idx = participantColorMap.size % SVG_BADGE_COLORS.length;
  const color = SVG_BADGE_COLORS[idx % SVG_BADGE_COLORS.length] ?? "#0040FF";
  participantColorMap.set(participantId, color);
  return color;
};

function* getNextPathIdGenerator(): Generator<number> {
  let index = 0;
  while (true) {
    yield index++;
    if (index >= Number.MAX_SAFE_INTEGER) {
      index = 0;
    }
  }
}
// Singleton generator
export const getNextPathId = getNextPathIdGenerator();
