"use client";

import { useEffect, useId, useMemo, useState } from "react";
import { geoGraticule, geoInterpolate, geoOrthographic, geoPath } from "d3-geo";
import { feature, mesh } from "topojson-client";
import worldAtlas from "world-atlas/land-110m.json";
import type { GeometryObject, Topology } from "topojson-specification";

const WORLD = worldAtlas as unknown as Topology;
const LAND = feature(WORLD, "land");
const COAST_MESH = mesh(WORLD, WORLD.objects.land as unknown as GeometryObject);
const NETWORK_POINTS: [number, number][] = [
  // North America, Central America, and the Caribbean.
  [-122, 37], [-118, 34], [-113, 51], [-112, 29], [-108, 40], [-105, 38], [-101, 49], [-101, 27], [-99, 19], [-96, 32], [-93, 36], [-90, 23], [-88, 47], [-88, 13], [-85, 34], [-84, 8], [-83, 20], [-80, 26], [-79, 9], [-77, 31], [-74, 40],
  // South America.
  [-79, 4], [-77, -12], [-75, -3], [-74, -12], [-70, -8], [-70, -20], [-70, -33], [-65, -14], [-61, -20], [-61, -6], [-58, -34], [-54, -31], [-51, -36], [-47, -16], [-46, -23],
  // Europe, Africa, and the Mediterranean.
  [-10, 52], [-3, 40], [-1, 52], [2, 48], [8, 51], [12, 42], [18, 59], [20, 45], [24, 38], [31, 30], [31, 6], [36, -1], [39, 9], [18, 15], [10, 5], [0, 7], [-1, 18], [18, -34], [28, -26],
  // Asia and Oceania.
  [41, 55], [46, 25], [55, 25], [67, 24], [72, 19], [77, 28], [78, 22], [90, 23], [103, 1], [105, 21], [110, 35], [116, 40], [121, 14], [127, 37], [139, 35], [144, 13], [151, -33], [133, -24], [115, -32],
];
const LOCAL_NETWORK_EDGES: [number, number][] = NETWORK_POINTS.flatMap((point, index) => (
  NETWORK_POINTS
    .map((candidate, candidateIndex) => ({ candidateIndex, distance: (point[0] - candidate[0]) ** 2 + (point[1] - candidate[1]) ** 2 }))
    .filter(({ candidateIndex }) => candidateIndex > index)
    .sort((left, right) => left.distance - right.distance)
    .slice(0, 3)
    .map(({ candidateIndex }) => [index, candidateIndex] as [number, number])
));
const NETWORK_EDGES: [number, number][] = [
  ...LOCAL_NETWORK_EDGES,
  // Long-distance paths remain sparse enough to read as a network, not a grid.
  [2, 14], [3, 18], [6, 24], [10, 28], [17, 39], [22, 43], [31, 49],
  [37, 53], [43, 59], [48, 65], [54, 70], [60, 76], [66, 81], [70, 84],
];

const DETAIL = {
  card: { gridStep: [15, 15] as [number, number], secondaryNodeEvery: 3 },
  hero: { gridStep: [10, 10] as [number, number], secondaryNodeEvery: 2 },
};

function greatCircle(from: [number, number], to: [number, number]) {
  const interpolate = geoInterpolate(from, to);
  return {
    type: "LineString" as const,
    coordinates: Array.from({ length: 17 }, (_, index) => interpolate(index / 16)),
  };
}

/**
 * A real orthographic globe. d3's longitude rotation moves coastlines, mesh,
 * and points together around the vertical/Y axis while the camera stays fixed.
 */
export function AnimatedGlobe({
  className = "",
  variant = "card",
}: {
  className?: string;
  variant?: "hero" | "card";
}) {
  const id = useId().replaceAll(":", "");
  const [longitude, setLongitude] = useState(75);
  const detail = DETAIL[variant];

  useEffect(() => {
    if (window.matchMedia("(prefers-reduced-motion: reduce)").matches) return;
    const startedAt = performance.now();
    let frame = 0;
    const rotate = (now: number) => {
      setLongitude(75 + ((now - startedAt) / 32_000) * 360);
      frame = requestAnimationFrame(rotate);
    };
    frame = requestAnimationFrame(rotate);
    return () => cancelAnimationFrame(frame);
  }, []);

  const drawing = useMemo(() => {
    const projection = geoOrthographic()
      .translate([50, 50])
      .scale(39.5)
      .rotate([longitude, -8])
      .clipAngle(90)
      .precision(0.15);
    const path = geoPath(projection);
    return {
      coast: path(LAND) ?? "",
      borders: path(COAST_MESH) ?? "",
      grid: path(geoGraticule().step(detail.gridStep)()) ?? "",
      links: NETWORK_EDGES.map(([from, to]) => path(greatCircle(NETWORK_POINTS[from], NETWORK_POINTS[to])) ?? ""),
      nodes: NETWORK_POINTS.map((point) => projection(point)),
    };
  }, [detail.gridStep, longitude]);

  return (
    <div className={`globe-3d globe-3d-${variant} ${className}`} aria-hidden="true">
      <svg className="globe-3d-art" viewBox="0 0 100 100" preserveAspectRatio="xMidYMid meet">
        <defs>
          <clipPath id={`${id}-clip`}><circle cx="50" cy="50" r="40" /></clipPath>
          <radialGradient id={`${id}-atmosphere`} cx="38%" cy="30%" r="72%">
            <stop offset="0%" stopColor="#0a3650" stopOpacity=".72" /><stop offset="58%" stopColor="#041722" stopOpacity=".94" /><stop offset="100%" stopColor="#01060b" />
          </radialGradient>
        </defs>
        <circle className="globe-3d-aura" cx="50" cy="50" r="44" />
        <g clipPath={`url(#${id}-clip)`}>
          <circle className="globe-3d-base" cx="50" cy="50" r="40" fill={`url(#${id}-atmosphere)`} />
          <path className="globe-3d-grid" d={drawing.grid} />
          <path className="globe-3d-coasts" d={drawing.coast} />
          <path className="globe-3d-borders" d={drawing.borders} />
          <g className="globe-3d-network">
            {drawing.links.map((link, index) => <path key={index} className={index >= LOCAL_NETWORK_EDGES.length ? "globe-3d-long-link" : undefined} d={link} />)}
          </g>
          <g className="globe-3d-nodes">
            {drawing.nodes.map((point, index) => point && <circle key={index} className={`globe-3d-node globe-3d-node-${index % 4}`} cx={point[0]} cy={point[1]} r={index % 7 === 0 ? 1 : .55} />)}
            {drawing.nodes.map((point, index) => point && index % detail.secondaryNodeEvery === 0 && <circle key={`secondary-${index}`} className="globe-3d-secondary-node" cx={point[0]} cy={point[1]} r=".28" />)}
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
      <div className="pointer-events-none mt-7 flex h-64 justify-center sm:h-80">
        <AnimatedGlobe className="h-64 w-64 shrink-0 sm:h-80 sm:w-80" />
      </div>
    </div>
  );
}
