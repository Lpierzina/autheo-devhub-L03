"use client";

/**
 * A CSS-only globe intentionally avoids a WebGL dependency: it works in the
 * dashboard's normal browser support envelope and retains a useful, still
 * outline when reduced motion is requested.
 *
 * The page background remains owned by globals.css. This component paints only
 * inside its own card, so its transitions cannot change the workshop artwork's
 * scale or position.
 */
export function AnimatedGlobe({ className = "" }: { className?: string }) {
  const nodes = [
    [18, 31], [31, 20], [48, 29], [66, 18], [79, 37],
    [23, 56], [41, 47], [57, 59], [72, 53], [37, 75], [61, 78],
  ];

  return (
    <div className={`globe-3d ${className}`} aria-hidden="true">
      <div className="globe-3d-aura" />
      <div className="globe-3d-orbit globe-3d-orbit-a" />
      <div className="globe-3d-orbit globe-3d-orbit-b" />
      <div className="globe-3d-sphere">
        <div className="globe-3d-inner-light" />
        <div className="globe-3d-grid globe-3d-grid-back">
          {[0, 1, 2, 3, 4].map((index) => <span key={`lat-${index}`} className={`globe-3d-latitude globe-3d-latitude-${index}`} />)}
          {[0, 1, 2, 3, 4].map((index) => <span key={`lon-${index}`} className={`globe-3d-longitude globe-3d-longitude-${index}`} />)}
        </div>
        <svg
          className="globe-3d-landmasses"
          viewBox="0 0 100 100"
          preserveAspectRatio="xMidYMid meet"
          aria-hidden="true"
        >
          <g className="globe-3d-landmass-fill">
            {/* Deliberately abstracted continental silhouettes: a readable world
                without turning the globe into a literal atlas. */}
            <path d="M12 29 18 21 28 19 33 23 32 29 38 33 35 39 28 39 25 44 18 42 15 36Z" />
            <path d="M33 47 39 48 42 55 40 62 44 69 40 80 35 75 34 65 30 57Z" />
            <path d="M48 26 55 20 66 21 71 27 79 29 84 35 80 40 69 39 63 45 55 42 49 37Z" />
            <path d="M57 48 66 46 73 52 72 61 68 67 67 77 62 81 57 72 53 62Z" />
            <path d="M80 69 87 72 89 78 84 83 78 80 77 74Z" />
          </g>
          <g className="globe-3d-landmass-cuts">
            <path d="m19 29 5 2-2 4-5-1Zm9-4 3 2-2 4-4-2Zm8 27 3 3-2 7-3-4Zm18-24 6 2-2 4-5-1Zm12 3 7 2-3 4-5-2Zm-5 22 5 2-2 5-4-2Zm2 14 4 2-1 6-4-2Z" />
          </g>
          <g className="globe-3d-coastlines">
            <path d="M12 29 18 21 28 19 33 23 32 29 38 33 35 39 28 39 25 44 18 42 15 36Z" />
            <path d="M33 47 39 48 42 55 40 62 44 69 40 80 35 75 34 65 30 57Z" />
            <path d="M48 26 55 20 66 21 71 27 79 29 84 35 80 40 69 39 63 45 55 42 49 37Z" />
            <path d="M57 48 66 46 73 52 72 61 68 67 67 77 62 81 57 72 53 62Z" />
            <path d="M80 69 87 72 89 78 84 83 78 80 77 74Z" />
          </g>
        </svg>
        <div className="globe-3d-network">
          <span className="globe-3d-path globe-3d-path-a" />
          <span className="globe-3d-path globe-3d-path-b" />
          <span className="globe-3d-path globe-3d-path-c" />
          {nodes.map(([left, top], index) => (
            <span
              key={`${left}-${top}`}
              className={`globe-3d-node globe-3d-node-${index % 3}`}
              style={{ left: `${left}%`, top: `${top}%` }}
            />
          ))}
        </div>
        <div className="globe-3d-grid globe-3d-grid-front">
          {[0, 1, 2].map((index) => <span key={`front-lat-${index}`} className={`globe-3d-latitude globe-3d-front-latitude-${index}`} />)}
          {[0, 1, 2].map((index) => <span key={`front-lon-${index}`} className={`globe-3d-longitude globe-3d-front-longitude-${index}`} />)}
        </div>
        <div className="globe-3d-glow" />
        <div className="globe-3d-rim" />
      </div>
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
