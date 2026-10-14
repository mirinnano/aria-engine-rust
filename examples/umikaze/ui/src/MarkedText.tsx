import type { MarkupSpan } from "@aria/ui-sdk";

const segmenter = new Intl.Segmenter("ja", { granularity: "grapheme" });

export function graphemes(text: string): string[] {
  return Array.from(segmenter.segment(text), (segment) => segment.segment);
}

/**
 * Renders a subtitle page with its ruby and emphasis. `visible` is how many
 * graphemes the typewriter has revealed; a ruby reading appears only once its
 * whole base is shown, so it never runs ahead of the words it annotates.
 */
export function MarkedText({ text, markup, visible }: {
  text: string;
  markup?: MarkupSpan[];
  visible?: number;
}) {
  const characters = graphemes(text);
  const shown = Math.min(visible ?? characters.length, characters.length);
  if (!markup?.length) return <>{characters.slice(0, shown).join("")}</>;

  const spans = [...markup].sort((left, right) => left.start - right.start);
  const parts: React.ReactNode[] = [];
  let cursor = 0;
  for (const span of spans) {
    if (span.start >= shown) break;
    if (span.start < cursor) continue;
    if (span.start > cursor) parts.push(characters.slice(cursor, span.start).join(""));
    const end = Math.min(span.end, shown);
    const base = characters.slice(span.start, end).join("");
    if (span.kind === "ruby") {
      parts.push(end === span.end
        ? <ruby key={span.start}>{base}<rp>(</rp><rt>{span.text}</rt><rp>)</rp></ruby>
        : base);
    } else {
      parts.push(<em key={span.start} className="text-emphasis">{base}</em>);
    }
    cursor = end;
  }
  if (cursor < shown) parts.push(characters.slice(cursor, shown).join(""));
  return <>{parts}</>;
}
