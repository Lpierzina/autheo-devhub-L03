"use client";

import { useEffect, useId, useMemo, useState } from "react";
import { geoGraticule10, geoInterpolate, geoOrthographic, geoPath } from "d3-geo";
import { feature, mesh } from "topojson-client";
import worldAtlas from "world-atlas/land-110m.json";
import type { GeometryObject, Topology } from "topojson-specification";

const WORLD = worldAtlas as unknown as Topology;
const LAND = feature(WORLD, "land");
const COAST_MESH = mesh(WORLD, WORLD.objects.land as unknown as GeometryObject);
const NETWORK_POINTS: [number, number][] = [
  [-122, 37], [-99, 19], [-74, 40], [-80, 26], [-79, 9], [-77, -12],
  [-58, -34], [-47, -16], [-70, -33], [-46, -23], [-3, 40], [2, 48],
  [18, 59], [31, 30], [55, 25], [77, 28], [103, 1], [139, 35],
  [-113, 51], [-101, 49], [-88, 47], [-116, 41], [-105, 38], [-93, 36],
  [-85, 34], [-77, 31], [-112, 29], [-101, 27], [-90, 23], [-83, 20],
  [-88, 13], [-84, 8], [-79, 4], [-75, -3], [-70, -8], [-65, -14],
  [-61, -20], [-57, -26], [-54, -31], [-51, -36], [-70, -20], [-61, -6],
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
  [2, 10], [3, 11], [4, 12], [5, 13], [6, 14], [7, 15], [9, 16],
];

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
      .precision(0.25);
    const path = geoPath(projection);
    return {
      coast: path(LAND) ?? "",
      borders: path(COAST_MESH) ?? "",
      grid: path(geoGraticule10()) ?? "",
      links: NETWORK_EDGES.map(([from, to]) => path(greatCircle(NETWORK_POINTS[from], NETWORK_POINTS[to])) ?? ""),
      nodes: NETWORK_POINTS.map((point) => projection(point)),
    };
  }, [longitude]);

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
