"use client";

import Link from "next/link";
import { ArrowRight, CircleHelp, LockKeyhole, Radio, ShieldCheck } from "lucide-react";
import type { ReactNode } from "react";
import { usePoll } from "@/lib/api";

type Evidence = {
  state?: "enabled" | "unavailable" | "unknown";
  detail?: string;
  observed_at_ms?: number;
};

type SecurityPosture = {
  observed_at_ms?: number;
  post_quantum?: Evidence;
  network?: Evidence;
  isolation?: Evidence;
};

const stateLabel = (state?: Evidence["state"]) => {
  if (state === "enabled") return "Observed";
  if (state === "unavailable") return "Unavailable";
  return "Unknown";
};

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
  const state = pq?.state;
  const unavailable = !data || state === "unavailable";
  const label = unavailable ? "Security profile unavailable" : "Post-quantum cryptography";
  const detail = pq?.detail ?? (error
    ? "No evidence report received from this control plane."
    : "Waiting for an observed security capability report.");

  return (
    <Link
      href="/settings/security"
      className="mb-6 flex items-center gap-3 rounded-xl border border-border bg-card px-4 py-3 shadow-sm transition-shadow hover:shadow-pop"
    >
      <span className={`flex h-9 w-9 shrink-0 items-center justify-center rounded-full ${unavailable ? "bg-subtle text-secondary" : "bg-emerald-500/10 text-emerald-600 dark:text-emerald-300"}`}>
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
          Observed capabilities from the control plane. Unknown means this node has not supplied evidence; it is not an assurance.
        </p>
      </div>
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
