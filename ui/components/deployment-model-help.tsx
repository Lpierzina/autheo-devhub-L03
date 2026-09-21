"use client";

import { useId, useState } from "react";
import { ChevronDown, Info } from "lucide-react";
import { cn } from "@/lib/utils";

/**
 * A concise explanation of the declarative deployment model, shared by every
 * Add Project entry point so a source type does not change the guidance.
 */
export function DeploymentModelHelp({ className }: { className?: string }) {
  const [open, setOpen] = useState(false);
  const contentId = useId();

  return (
    <section className={cn("overflow-hidden rounded-lg border border-emerald-500/30 bg-card", className)}>
      <button
        type="button"
        onClick={() => setOpen((value) => !value)}
        aria-expanded={open}
        aria-controls={contentId}
        className="flex w-full items-center justify-between gap-3 px-3 py-2.5 text-left hover:bg-emerald-500/5"
      >
        <span className="flex min-w-0 items-center gap-2">
          <Info className="h-4 w-4 shrink-0 text-emerald-600 dark:text-emerald-400" />
          <span>
            <span className="block text-sm font-medium text-fg">How DevHub knows what to run</span>
            <span className="block text-xs text-secondary">Configuration, services, and managed storage</span>
          </span>
        </span>
        <ChevronDown className={cn("h-4 w-4 shrink-0 text-muted transition-transform", open ? "" : "-rotate-90")} />
      </button>

      {open && (
        <div id={contentId} className="border-t border-emerald-500/20 px-3 py-3 text-xs leading-5 text-secondary">
          <p className="text-fg">
            DevHub uses explicit project configuration. It does not inspect your application code and guess which services you need.
          </p>

          <div className="mt-3 space-y-3">
            <div>
              <h3 className="font-medium text-fg"><code>fluid.json</code></h3>
              <p>Use this for a static frontend plus functions or routes. It declares application entrypoints, runtimes, and routes for frontend/API-style projects whose services are functions.</p>
            </div>

            <div>
              <h3 className="font-medium text-fg"><code>compose.yaml</code> / Docker Compose</h3>
              <p>Use this for multiple processes or containers, such as a frontend, API, worker, Redis, or other sidecars. For supported Compose deployments, DevHub reads the declared services and runs them in the project&apos;s private service network. The primary web service is exposed by default; supporting services stay private unless explicitly configured for exposure.</p>
              <p className="mt-1 text-muted">Have services retry dependency connections during startup.</p>
            </div>

            <div>
              <h3 className="font-medium text-fg">Managed storage</h3>
              <p>Create and attach supported managed stores through DevHub Storage. Attached Postgres and Redis stores provide runtime connection configuration such as <code>DATABASE_URL</code> or <code>REDIS_URL</code>. An external or Compose-managed MongoDB must be declared and configured by you.</p>
            </div>
          </div>

          <p className="mt-3 border-t border-emerald-500/20 pt-3 text-fg">
            Declare the services your app needs in <code>fluid.json</code> or <code>compose.yaml</code>; attach supported managed stores to the project. DevHub deploys that declared topology rather than guessing from source code.
          </p>
        </div>
      )}
    </section>
  );
}
