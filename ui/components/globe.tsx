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
