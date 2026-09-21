"use client";

import { useId } from "react";

/**
 * A projection-style SVG Earth. The map shifts through longitudes beneath a
 * fixed spherical grid; this is an actual horizontal world rotation rather
 * than a 2D spin of the circular canvas.
 */
export function AnimatedGlobe({
  className = "",
  variant = "card",
}: {
  className?: string;
  variant?: "hero" | "card";
}) {
  const id = useId().replaceAll(":", "");
  const coastlines = (
    <>
      <path d="M14 31 16 27 20 26 21 23 25 21 28 18 34 18 38 21 42 23 43 27 40 29 41 33 38 35 35 34 32 37 29 36 27 40 24 39 22 43 19 42 18 38 15 37Z M28 42 31 44 32 47 35 49 35 52 33 53 31 50 29 48Z M35 16 39 12 43 13 45 16 43 19 39 19Z" />
      <path d="M35 53 39 51 43 53 45 57 44 61 46 65 44 70 45 74 42 79 40 85 37 82 36 77 34 73 35 69 32 64 32 60 34 57Z" />
      <path d="M48 28 51 24 55 24 57 21 61 22 63 25 68 24 71 26 76 26 79 28 84 29 87 32 91 34 90 38 86 40 82 39 79 42 75 40 71 43 67 42 64 45 59 43 56 45 53 42 52 38 48 36Z" />
      <path d="M57 47 61 44 65 46 69 49 71 53 69 57 68 62 66 65 67 70 64 75 64 81 61 85 58 80 56 75 54 71 55 66 52 62 53 57 51 54 53 50Z" />
      <path d="M83 45 85 44 86 47 84 49Z M78 57 81 57 83 60 81 62 78 61Z M82 64 86 65 88 68 86 70 83 68Z M79 73 84 72 89 75 91 79 88 83 83 83 78 79Z M46 32 47 30 48 31 47 33Z M72 48 73 47 74 48 73 49Z" />
    </>
  );
  const mesh = (
    <>
      <path d="M15 32 24 28 31 34 39 26 48 33 56 28 64 35 74 30 86 36 M18 42 27 39 35 48 44 41 52 47 61 40 70 48 80 42 88 50 M26 57 34 53 42 60 52 54 60 63 70 56 79 64 87 61 M34 72 42 68 48 75 56 70 64 78 73 72 84 77" />
      <path d="M24 28 27 39 18 42 M31 34 35 48 27 39 M39 26 44 41 35 48 M48 33 52 47 44 41 M56 28 61 40 52 47 M64 35 70 48 61 40 M74 30 80 42 70 48 M86 36 88 50 80 42 M35 48 34 53 26 57 M44 41 42 60 34 53 M52 47 52 54 42 60 M61 40 60 63 52 54 M70 48 70 56 60 63 M80 42 79 64 70 56 M88 50 87 61 79 64 M26 57 34 72 M34 53 42 68 M42 60 48 75 M52 54 56 70 M60 63 64 78 M70 56 73 72 M79 64 84 77" />
      <path className="globe-3d-long-link" d="M20 41 Q48 13 84 35 M34 72 Q56 87 83 60 M27 39 Q56 57 80 42" />
    </>
  );
  const nodes = [[20, 42], [24, 28], [31, 34], [35, 48], [39, 26], [42, 60], [48, 33], [52, 54], [56, 28], [60, 63], [64, 35], [70, 48], [74, 30], [79, 64], [80, 42], [86, 36], [88, 50]];

  return (
    <div className={`globe-3d globe-3d-${variant} ${className}`} aria-hidden="true">
      <svg className="globe-3d-art" viewBox="0 0 100 100" preserveAspectRatio="xMidYMid meet">
        <defs>
          <clipPath id={`${id}-clip`}><circle cx="50" cy="50" r="40" /></clipPath>
          <radialGradient id={`${id}-atmosphere`} cx="38%" cy="30%" r="72%">
            <stop offset="0%" stopColor="#0a3650" stopOpacity=".72" /><stop offset="58%" stopColor="#041722" stopOpacity=".94" /><stop offset="100%" stopColor="#01060b" />
          </radialGradient>
          <linearGradient id={`${id}-limb-fade`} x1="0" x2="1"><stop offset="0%" stopColor="black" /><stop offset="15%" stopColor="white" /><stop offset="76%" stopColor="white" /><stop offset="100%" stopColor="black" /></linearGradient>
          <mask id={`${id}-hemisphere`}><circle cx="50" cy="50" r="40" fill={`url(#${id}-limb-fade)`} /></mask>
        </defs>
        <circle className="globe-3d-aura" cx="50" cy="50" r="44" />
        <g clipPath={`url(#${id}-clip)`}>
          <circle className="globe-3d-base" cx="50" cy="50" r="40" fill={`url(#${id}-atmosphere)`} />
          <g className="globe-3d-grid"><ellipse cx="50" cy="24" rx="22" ry="5" /><ellipse cx="50" cy="34" rx="34" ry="8" /><ellipse cx="50" cy="50" rx="40" ry="10" /><ellipse cx="50" cy="66" rx="34" ry="8" /><ellipse cx="50" cy="76" rx="22" ry="5" /><ellipse cx="50" cy="50" rx="11" ry="40" /><ellipse cx="50" cy="50" rx="24" ry="40" /><ellipse cx="50" cy="50" rx="35" ry="40" /></g>
          <g mask={`url(#${id}-hemisphere)`}>
            <g className="globe-3d-rotor">
              {[0, 80].map((offset) => <g key={offset} transform={`translate(${offset} 0)`}><g className="globe-3d-coasts">{coastlines}</g><g className="globe-3d-network">{mesh}</g><g className="globe-3d-nodes">{nodes.map(([cx, cy], index) => <circle key={`${offset}-${cx}-${cy}`} className={`globe-3d-node globe-3d-node-${index % 4}`} cx={cx} cy={cy} r={index % 7 === 0 ? 1 : .55} />)}</g></g>)}
            </g>
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
