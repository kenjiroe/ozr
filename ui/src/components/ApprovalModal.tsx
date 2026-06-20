import type { PendingApproval } from "../api/ozrClient";

interface ApprovalModalProps {
  pending: PendingApproval;
  busy: boolean;
  onApprove: () => void;
  onDeny: () => void;
}

function riskTone(actionKind: string, riskTier: string) {
  const kind = actionKind.toLowerCase();
  if (kind === "shell" || kind === "network" || riskTier === "high") {
    return {
      badge: "bg-red-500/20 text-red-200 ring-red-400/40",
      panel: "border-red-500/40 bg-red-950/40",
      label: "High risk",
    };
  }
  if (kind === "write" || riskTier === "medium") {
    return {
      badge: "bg-amber-500/20 text-amber-100 ring-amber-400/40",
      panel: "border-amber-500/40 bg-amber-950/30",
      label: "Medium risk",
    };
  }
  return {
    badge: "bg-emerald-500/20 text-emerald-100 ring-emerald-400/40",
    panel: "border-emerald-500/40 bg-emerald-950/30",
    label: "Low risk",
  };
}

export function ApprovalModal({
  pending,
  busy,
  onApprove,
  onDeny,
}: ApprovalModalProps) {
  const tone = riskTone(pending.action_kind, pending.risk_tier);

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/70 p-4 backdrop-blur-sm">
      <div
        className={`w-full max-w-lg rounded-2xl border p-6 shadow-2xl ${tone.panel}`}
        role="dialog"
        aria-modal="true"
        aria-labelledby="approval-title"
      >
        <div className="mb-4 flex flex-wrap items-center gap-2">
          <span
            className={`rounded-full px-3 py-1 text-xs font-semibold uppercase tracking-wide ring-1 ${tone.badge}`}
          >
            {tone.label}
          </span>
          <span className="rounded-full bg-slate-800 px-3 py-1 font-mono text-xs text-slate-200 ring-1 ring-slate-600">
            [{pending.action_kind}]
          </span>
          <span className="rounded-full bg-slate-800 px-3 py-1 font-mono text-xs text-slate-300 ring-1 ring-slate-600">
            tool: {pending.tool}
          </span>
        </div>

        <h2 id="approval-title" className="text-xl font-semibold text-white">
          Approval required
        </h2>
        <p className="mt-2 text-sm text-slate-300">
          ozr paused this run at the Plan Mode guardrail. Review the planned action
          before allowing side effects.
        </p>

        <dl className="mt-4 space-y-2 rounded-xl bg-black/30 p-4 text-sm">
          <div className="flex justify-between gap-4">
            <dt className="text-slate-400">Plan ID</dt>
            <dd className="font-mono text-slate-100">{pending.plan_id}</dd>
          </div>
          <div className="flex justify-between gap-4">
            <dt className="text-slate-400">Risk tier</dt>
            <dd className="font-mono uppercase text-slate-100">{pending.risk_tier}</dd>
          </div>
          <div>
            <dt className="text-slate-400">Params</dt>
            <dd className="mt-1 break-all font-mono text-xs text-slate-200">
              {pending.params}
            </dd>
          </div>
        </dl>

        <div className="mt-6 flex flex-col gap-3 sm:flex-row sm:justify-end">
          <button
            type="button"
            disabled={busy}
            onClick={onDeny}
            className="rounded-xl border border-slate-600 bg-slate-900 px-4 py-2.5 text-sm font-medium text-slate-100 transition hover:bg-slate-800 disabled:opacity-50"
          >
            Deny / Abort
          </button>
          <button
            type="button"
            disabled={busy}
            onClick={onApprove}
            className="rounded-xl bg-emerald-600 px-4 py-2.5 text-sm font-semibold text-white transition hover:bg-emerald-500 disabled:opacity-50"
          >
            Approve &amp; Run
          </button>
        </div>
      </div>
    </div>
  );
}
