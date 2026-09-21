"use client";

import { useId } from "react";

/**
 * An inline SVG globe avoids a WebGL dependency while keeping the stencil,
 * technical linework, and a useful still outline for reduced motion.
 *
 * The page background remains owned by globals.css. This component paints only
 * inside its own card, so its transitions cannot change the workshop artwork's
 * scale or position.
 */
export function AnimatedGlobe({
  className = "",
  variant = "card",
}: {
  className?: string;
  variant?: "hero" | "card";
}) {
  const isHero = variant === "hero";
  const id = useId().replaceAll(":", "");

  return (
    <div className={`globe-3d globe-3d-${variant} ${className}`} aria-hidden="true">
      <svg className="globe-3d-art" viewBox="0 0 100 100" preserveAspectRatio="xMidYMid meet">
        <defs>
          <clipPath id={`${id}-clip`}><circle cx="50" cy="50" r="40" /></clipPath>
          <radialGradient id={`${id}-atmosphere`} cx="34%" cy="28%" r="72%">
            <stop offset="0%" stopColor="var(--globe-mint)" stopOpacity=".18" />
            <stop offset="66%" stopColor="var(--globe-green)" stopOpacity=".025" />
            <stop offset="100%" stopColor="var(--globe-blue)" stopOpacity=".1" />
          </radialGradient>
          <mask id={`${id}-continent-stencil`}>
            <rect width="100" height="100" fill="black" />
            {/* Abstract continental silhouettes: a readable world, never an atlas. */}
            <path fill="white" d="M14 34 18 25 28 21 35 24 35 30 41 33 38 40 31 41 27 47 19 44 15 39Z M31 49 39 49 43 56 40 64 45 70 40 81 35 76 34 67 29 58Z M47 28 55 21 66 22 72 27 80 29 86 36 82 42 72 41 65 46 56 43 49 38Z M56 49 65 47 73 53 72 62 68 68 67 79 62 83 56 73 52 63Z M80 70 88 73 90 79 85 84 78 81 77 75Z" />
            {/* Slots turn the land layer into a transparent technical stencil. */}
            <path fill="black" d="M19 30h8v2h-8Zm10-4h4v3h-4Zm-1 12h8v2h-8Zm7 16h5v3h-5Zm2 10h5v2h-5Zm15-34h8v2h-8Zm13-2h9v3h-9Zm5 11h8v2h-8Zm-8 15h7v3h-7Zm0 12h6v2h-6Zm18 10h6v2h-6Z" />
          </mask>
        </defs>

        <circle className="globe-3d-aura" cx="50" cy="50" r="46" />
        <g className="globe-3d-orbits">
          <ellipse className="globe-3d-orbit globe-3d-orbit-back" cx="50" cy="50" rx="48" ry="13" />
          {isHero && <ellipse className="globe-3d-orbit globe-3d-orbit-front" cx="50" cy="50" rx="54" ry="20" />}
        </g>
        <g clipPath={`url(#${id}-clip)`}>
          <circle className="globe-3d-base" cx="50" cy="50" r="40" fill={`url(#${id}-atmosphere)`} />
          <g className="globe-3d-grid globe-3d-grid-far">
            <ellipse cx="50" cy="27" rx="25" ry="7" /><ellipse cx="50" cy="39" rx="36" ry="9" />
            <ellipse cx="50" cy="51" rx="40" ry="10" /><ellipse cx="50" cy="63" rx="35" ry="9" />
            <ellipse cx="50" cy="75" rx="24" ry="7" />
            <ellipse cx="50" cy="50" rx="13" ry="40" /><ellipse cx="50" cy="50" rx="27" ry="40" /><ellipse cx="50" cy="50" rx="39" ry="40" />
          </g>
          <g className="globe-3d-world" mask={`url(#${id}-continent-stencil)`}><rect x="10" y="10" width="80" height="80" /></g>
          <g className="globe-3d-coasts">
            <path d="M14 34 18 25 28 21 35 24 35 30 41 33 38 40 31 41 27 47 19 44 15 39Z M31 49 39 49 43 56 40 64 45 70 40 81 35 76 34 67 29 58Z M47 28 55 21 66 22 72 27 80 29 86 36 82 42 72 41 65 46 56 43 49 38Z M56 49 65 47 73 53 72 62 68 68 67 79 62 83 56 73 52 63Z M80 70 88 73 90 79 85 84 78 81 77 75Z" />
          </g>
          <g className="globe-3d-network globe-3d-network-far"><path d="M17 38 Q43 8 79 35" /><path d="M28 59 Q57 77 82 43" /></g>
          <g className="globe-3d-grid globe-3d-grid-front">
            <ellipse cx="50" cy="51" rx="40" ry="10" /><ellipse cx="50" cy="63" rx="35" ry="9" />
            <ellipse cx="50" cy="50" rx="13" ry="40" /><ellipse cx="50" cy="50" rx="27" ry="40" />
          </g>
          <g className="globe-3d-network globe-3d-network-front">
            <path d="M19 42 Q46 68 80 37" /><path d="M26 58 Q47 35 71 54" />
            {[[20, 42], [31, 28], [43, 50], [57, 35], [70, 54], [80, 37], [62, 70]].map(([cx, cy], index) => (
              <circle key={`${cx}-${cy}`} className={`globe-3d-node globe-3d-node-${index % 3}`} cx={cx} cy={cy} r={index === 2 || index === 5 ? 1.5 : 1} />
            ))}
          </g>
        </g>
        <circle className="globe-3d-rim" cx="50" cy="50" r="40" />
      </svg>
    </div>
  );
}

/** Animated wireframe empty state, anchored to the bottom of its card. */
export function GlobeEmptyState({ title, desc }: { title: string; desc?: string }) {
  return (
    <div className="relative overflow-hidden">
      {(title || desc) && (
        <div className="relative z-10 pt-2">
          {title ? <h2 className="text-2xl font-semibold tracking-tight text-fg">{title}</h2> : null}
          {desc ? <p className="mt-1.5 text-sm text-secondary">{desc}</p> : null}
        </div>
      )}
      <div className="pointer-events-none mt-6 flex h-52 items-end justify-center sm:h-64">
        <AnimatedGlobe className="h-64 w-64 shrink-0 sm:h-80 sm:w-80" />
      </div>
    </div>
  );
}
