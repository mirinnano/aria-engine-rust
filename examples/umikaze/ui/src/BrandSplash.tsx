import { useEffect, useRef, useState } from "react";
import makerLogo from "./assets/brand/logo-badge-line-white.svg";
import gameLogo from "./assets/brand/hesphoros-logo-white.svg";

/** Logos are the maker's own marks, fetched from ponkotsu-soft.ponkotu.workers.dev. */
const cards = [
  { id: "maker", source: makerLogo, label: "PONKOTSU software（ぽんこつソフト）", hold: 1400 },
  { id: "game", source: gameLogo, label: "HesPhoros", hold: 1700 },
] as const;

const FADE_MS = 700;
const GAP_MS = 280;

// "pre" is one hidden frame so the image mounts transparent and can fade in.
type Phase = "pre" | "in" | "hold" | "out" | "gap";

/**
 * The launch card sequence: maker mark, then title mark, each fading in and
 * out over black. Any key or pointer press moves to the next card. Automated
 * runs skip it unless the URL asks for `?splash=1`, so behaviour tests start
 * at the title they describe.
 */
export function shouldShowBrandSplash(): boolean {
  const query = new URLSearchParams(window.location.search);
  if (query.get("splash") === "1") return true;
  if (query.get("splash") === "0") return false;
  return !navigator.webdriver;
}

export function BrandSplash({ reducedMotion, onDone }: { reducedMotion: boolean; onDone: () => void }) {
  const [index, setIndex] = useState(0);
  const [phase, setPhase] = useState<Phase>("pre");
  const done = useRef(onDone);
  done.current = onDone;
  const fade = reducedMotion ? 0 : FADE_MS;

  useEffect(() => {
    const card = cards[index];
    const delay = { pre: 34, in: fade, hold: card.hold, out: fade, gap: GAP_MS }[phase];
    const timer = window.setTimeout(() => {
      if (phase === "pre") setPhase("in");
      else if (phase === "in") setPhase("hold");
      else if (phase === "hold") setPhase("out");
      else if (phase === "out") setPhase("gap");
      else if (index + 1 < cards.length) {
        setIndex(index + 1);
        setPhase("pre");
      } else {
        done.current();
      }
    }, delay);
    return () => window.clearTimeout(timer);
  }, [index, phase, fade]);

  useEffect(() => {
    const skip = (event: Event) => {
      event.preventDefault();
      event.stopPropagation();
      // A press ends the visible card; the sequence then continues normally.
      setPhase((current) => (current === "in" || current === "hold" ? "out" : current));
    };
    window.addEventListener("keydown", skip, { capture: true });
    window.addEventListener("pointerdown", skip, { capture: true });
    return () => {
      window.removeEventListener("keydown", skip, { capture: true });
      window.removeEventListener("pointerdown", skip, { capture: true });
    };
  }, []);

  const card = cards[index];
  const visible = phase === "in" || phase === "hold";
  return (
    <div className="brand-splash" data-card={card.id} role="presentation">
      <img
        key={card.id}
        className={`brand-splash-logo brand-splash-logo--${card.id}${visible ? " is-visible" : ""}`}
        src={card.source}
        alt={card.label}
        style={{ transitionDuration: `${fade}ms` }}
        draggable={false}
      />
    </div>
  );
}
