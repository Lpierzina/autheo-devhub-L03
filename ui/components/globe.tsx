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
  return (
    <div className={`globe-3d ${className}`} aria-hidden="true">
      <div className="globe-3d-orbit globe-3d-orbit-a" />
      <div className="globe-3d-orbit globe-3d-orbit-b" />
      <div className="globe-3d-sphere">
        <div className="globe-3d-grid">
          {[0, 1, 2, 3, 4].map((index) => <span key={`lat-${index}`} className={`globe-3d-latitude globe-3d-latitude-${index}`} />)}
          {[0, 1, 2, 3, 4].map((index) => <span key={`lon-${index}`} className={`globe-3d-longitude globe-3d-longitude-${index}`} />)}
        </div>
        <div className="globe-3d-glow" />
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
