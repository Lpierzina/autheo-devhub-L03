"use client";

import Link from "next/link";
import { ArrowRight, CircleHelp, LockKeyhole, Radio, ShieldCheck, UsersRound } from "lucide-react";
import type { ReactNode } from "react";
import { usePoll } from "@/lib/api";

export type EvidenceState = "enabled" | "partial" | "unavailable" | "unknown";

export type SecurityEvidence = {
  state?: EvidenceState;
  detail?: string;
  observed_at_ms?: number;
};

export type SecurityPosture = {
  observed_at_ms?: number;
  observer?: { node?: string; region?: string };
  post_quantum?: SecurityEvidence;
  network?: SecurityEvidence;
  isolation?: SecurityEvidence & {
    backend_counts?: { firecracker?: number; litebox?: number; mock?: number; unknown?: number };
    observer_backend?: string;
  };
};

const stateLabel = (state?: SecurityEvidence["state"]) => {
  if (state === "enabled") return "Enabled";
  if (state === "partial") return "Partial";
  if (state === "unavailable") return "Unavailable";
  return "Unknown";
};

/**
 * The posture endpoint is the only authority for runtime security evidence.
 * In particular, a PQ label must remain false unless its negotiated-session
 * evidence is reported as enabled by this contract.
 */
export function isPostQuantumProtected(posture?: SecurityPosture | null): boolean {
  return posture?.post_quantum?.state === "enabled";
}

export function useSecurityPosture() {
  return usePoll<SecurityPosture>("/v1/security/posture", 30_000);
}

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
  fallbackDetail,
  children,
}: {
  icon: ReactNode;
  label: string;
  evidence?: SecurityEvidence;
  fallbackDetail?: string;
  children?: ReactNode;
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
          {evidence?.detail ?? fallbackDetail ?? "No completed evidence has been reported by this control plane."}
        </p>
        {children}
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
  const { data, error } = useSecurityPosture();
  const posture = error ? null : data;
  const pq = posture?.post_quantum;
  const unavailable = !posture;
  const hasControls = posture?.network?.state === "enabled" || posture?.network?.state === "partial"
    || posture?.isolation?.state === "enabled" || posture?.isolation?.state === "partial";
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
      href="/network"
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

export function SecurityProfile({
  posture,
  error,
  loading,
}: {
  posture: SecurityPosture | null;
  error?: string | null;
  loading?: boolean;
}) {
  const hasRequiredEvidence = Boolean(
    posture?.network && posture?.post_quantum && posture?.isolation,
  );
  const evidenceUnavailable = Boolean(error) || !hasRequiredEvidence;
  const counts = posture?.isolation?.backend_counts;

  return (
    <section className="mt-8 border-t border-border pt-8">
      <div className="mb-4">
        <h2 className="text-xl font-semibold tracking-tight text-accent">Security Profile</h2>
        <p className="mt-1 text-sm text-secondary">
          Evidence-only capability report. Enabled means direct runtime evidence; Partial means a real control with incomplete coverage or assurance; Unavailable means known disabled; Unknown means required evidence is missing.
        </p>
      </div>
      {posture?.observer && (
        <p className="mb-4 text-xs text-muted">
          Responding node: {posture.observer.node ?? "unknown"} / {posture.observer.region ?? "unknown region"}. This report is computed by that control-plane node from its current mesh view and is not a fleet-wide session measurement or placement guarantee.
        </p>
      )}
      <div className="grid gap-3 lg:grid-cols-2">
        <EvidenceRow icon={<Radio className="h-5 w-5" />} label="Transport security" evidence={posture?.network} />
        <EvidenceRow
          icon={<UsersRound className="h-5 w-5" />}
          label="Peer admission policy"
          fallbackDetail="This posture contract does not provide separate peer admission policy evidence."
        />
        <EvidenceRow icon={<ShieldCheck className="h-5 w-5" />} label="Workload isolation" evidence={posture?.isolation}>
          {counts && (
            <p className="mt-2 text-xs text-muted">
              Observed backend counts: Firecracker {counts.firecracker ?? 0}, Litebox {counts.litebox ?? 0}, Mock {counts.mock ?? 0}, unknown {counts.unknown ?? 0}.
            </p>
          )}
        </EvidenceRow>
        <EvidenceRow icon={<LockKeyhole className="h-5 w-5" />} label="Post-quantum handshake evidence" evidence={posture?.post_quantum} />
      </div>
      <p className="mt-5 text-xs text-muted">
        {loading
          ? "Requesting the current evidence report..."
          : evidenceUnavailable
            ? "Security posture evidence is currently unavailable from this control plane."
            : "Reported values are refreshed automatically."}
      </p>
    </section>
  );
}
