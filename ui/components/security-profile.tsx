"use client";

import Link from "next/link";
import { ArrowRight, CircleHelp, LockKeyhole, Radio, ShieldCheck } from "lucide-react";
import type { ReactNode } from "react";
import { usePoll } from "@/lib/api";

type EvidenceState = "enabled" | "partial" | "unavailable" | "unknown";

type Evidence = {
  state?: EvidenceState;
  detail?: string;
  observed_at_ms?: number;
};

type SecurityPosture = {
  observed_at_ms?: number;
  observer?: { node?: string; region?: string };
  post_quantum?: Evidence;
  network?: Evidence;
  isolation?: Evidence & {
    backend_counts?: { firecracker?: number; litebox?: number; mock?: number; unknown?: number };
    observer_backend?: string;
  };
};

const stateLabel = (state?: Evidence["state"]) => {
  if (state === "enabled") return "Enabled";
  if (state === "partial") return "Partial";
  if (state === "unavailable") return "Unavailable";
  return "Unknown";
};

function observationTime(observedAt?: number) {
  if (!observedAt) return "Observation time unavailable";
  const elapsedSeconds = Math.max(0, Math.floor((Date.now() - observedAt) / 1000));
  if (elapsedSeconds < 10) return "Observed just now";
  if (elapsedSeconds < 60) return `Observed ${elapsedSeconds}s ago`;
  const elapsedMinutes = Math.floor(elapsedSeconds / 60);
  if (elapsedMinutes < 60) return `Observed ${elapsedMinutes}m ago`;
  return `Observed ${new Date(observedAt).toLocaleString()}`;
}

function EvidenceRow({
  icon,
  label,
  evidence,
}: {
  icon: ReactNode;
  label: string;
  evidence?: Evidence;
}) {
  const state = evidence?.state;
  const tone = state === "enabled"
    ? "border-emerald-500/30 bg-emerald-500/10 text-emerald-700 dark:text-emerald-300"
    : state === "partial"
      ? "border-blue-500/30 bg-blue-500/10 text-blue-700 dark:text-blue-300"
    : state === "unavailable"
        ? "border-amber-500/30 bg-amber-500/10 text-amber-700 dark:text-amber-300"
        : "border-border bg-subtle text-secondary";
  return (
    <div className="flex items-start gap-3 rounded-lg border border-border bg-card p-4">
      <span className="mt-0.5 text-accent">{icon}</span>
      <div className="min-w-0 flex-1">
        <div className="font-medium">{label}</div>
        <p className="mt-1 text-sm text-secondary">
          {evidence?.detail ?? "No completed evidence has been reported by this control plane."}
        </p>
        <p className="mt-2 text-xs text-muted">{observationTime(evidence?.observed_at_ms)}</p>
      </div>
      <span className={`shrink-0 rounded-full border px-2 py-0.5 text-xs font-medium ${tone}`}>
        {stateLabel(state)}
      </span>
    </div>
  );
}

/**
 * Security state is deliberately evidence-only. Older control planes do not
 * implement this endpoint, and a failed request is shown as Unknown rather
 * than guessed as either protected or unprotected.
 */
export function SecurityProfileBanner() {
  const { data, error } = usePoll<SecurityPosture>("/v1/security/posture", 30_000);
  const pq = data?.post_quantum;
  const unavailable = !data;
  const hasControls = data?.network?.state === "enabled" || data?.network?.state === "partial"
    || data?.isolation?.state === "enabled" || data?.isolation?.state === "partial";
  const label = unavailable
    ? "Security profile unavailable"
    : hasControls
      ? "Security profile available"
      : "Security profile available with unknown controls";
  const detail = pq?.state === "unavailable"
    ? "Post-quantum protection unavailable. Transport and isolation controls are reported separately."
    : pq?.detail ?? (error
    ? "No evidence report received from this control plane."
    : "Waiting for an observed security capability report.");

  return (
    <Link
      href="/settings/security"
      className="mb-6 flex items-center gap-3 rounded-xl border border-border bg-card px-4 py-3 shadow-sm transition-shadow hover:shadow-pop"
    >
      <span className={`flex h-9 w-9 shrink-0 items-center justify-center rounded-full ${unavailable ? "bg-subtle text-secondary" : hasControls ? "bg-emerald-500/10 text-emerald-600 dark:text-emerald-300" : "bg-blue-500/10 text-blue-600 dark:text-blue-300"}`}>
        {unavailable ? <CircleHelp className="h-4 w-4" /> : <ShieldCheck className="h-4 w-4" />}
      </span>
      <span className="min-w-0 flex-1">
        <span className="block text-sm font-semibold">{label}</span>
        <span className="block truncate text-xs text-secondary">{detail}</span>
      </span>
      <span className="flex shrink-0 items-center gap-1 text-sm font-medium text-link">
        View security profile <ArrowRight className="h-4 w-4" />
      </span>
    </Link>
  );
}

export function SecurityProfile() {
  const { data, error, loading } = usePoll<SecurityPosture>("/v1/security/posture", 30_000);
  return (
    <section>
      <div className="mb-6">
        <h1 className="text-3xl font-semibold tracking-tight text-accent">Security Profile</h1>
        <p className="mt-1 text-sm text-secondary">
          Evidence-only capability report. Enabled means direct runtime evidence; Partial means a real control with incomplete coverage or assurance; Unavailable means known disabled; Unknown means required evidence is missing.
        </p>
      </div>
      {data?.observer && (
        <p className="mb-4 text-xs text-muted">
          Responding node: {data.observer.node ?? "unknown"} · {data.observer.region ?? "unknown region"}. This report is computed by that control-plane node from its current mesh view and is not a placement guarantee.
        </p>
      )}
      <div className="grid gap-3">
        <EvidenceRow icon={<LockKeyhole className="h-5 w-5" />} label="Post-quantum cryptography" evidence={data?.post_quantum} />
        <EvidenceRow icon={<Radio className="h-5 w-5" />} label="Transport security" evidence={data?.network} />
        <EvidenceRow icon={<ShieldCheck className="h-5 w-5" />} label="Workload isolation" evidence={data?.isolation} />
      </div>
      <p className="mt-5 text-xs text-muted">
        {loading ? "Requesting the current evidence report…" : error ? "This control plane does not currently provide a security profile endpoint." : "Reported values are refreshed automatically."}
      </p>
    </section>
  );
}
