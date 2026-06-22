import { FormEvent, useCallback, useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  OzrApiClient,
  PendingApproval,
  pollSessionUntil,
  SessionView,
} from "../api/ozrClient";
import { ApprovalModal } from "./ApprovalModal";
import { ChatEntry, ChatPanel } from "./ChatPanel";

type RunPhase = "idle" | "running" | "awaiting_approval" | "approving";

interface ApiBootInfo {
  base_url: string;
  mode: "spawned" | "external";
}

export function Workspace() {
  const [apiBase, setApiBase] = useState<string | null>(null);
  const [bootError, setBootError] = useState<string | null>(null);
  const [prompt, setPrompt] = useState("");
  const [messages, setMessages] = useState<ChatEntry[]>([]);
  const [phase, setPhase] = useState<RunPhase>("idle");
  const [sessionId, setSessionId] = useState<string | null>(null);
  const [pending, setPending] = useState<PendingApproval | null>(null);
  const [statusLine, setStatusLine] = useState("Connecting to ozr API…");

  const client = useMemo(
    () => (apiBase ? new OzrApiClient(apiBase) : null),
    [apiBase],
  );

  const pushMessage = useCallback((role: ChatEntry["role"], content: string) => {
    setMessages((current) => [
      ...current,
      { id: `${Date.now()}-${Math.random()}`, role, content },
    ]);
  }, []);

  useEffect(() => {
    let cancelled = false;

    async function boot() {
      try {
        const bootInfo = await invoke<ApiBootInfo>("prepare_api");
        if (cancelled) {
          return;
        }
        const label =
          bootInfo.mode === "external" ? "external API" : "local spawn";
        setApiBase(bootInfo.base_url);
        setStatusLine(`Connected (${label}) · ${bootInfo.base_url}`);
        setBootError(null);
      } catch (error) {
        if (!cancelled) {
          setBootError(error instanceof Error ? error.message : String(error));
          setStatusLine("Failed to connect to ozr API");
        }
      }
    }

    void boot();
    return () => {
      cancelled = true;
    };
  }, []);

  const handleSubmit = async (event: FormEvent) => {
    event.preventDefault();
    if (!client || phase !== "idle" || !prompt.trim()) {
      return;
    }

    const userPrompt = prompt.trim();
    setPrompt("");
    pushMessage("user", userPrompt);
    setPhase("running");
    setStatusLine("Run started…");
    let waitingForApproval = false;

    try {
      const run = await client.run(userPrompt);
      setSessionId(run.session_id);
      setStatusLine(`Session ${run.session_id} · running`);

      const view = await pollSessionUntil(
        client,
        run.session_id,
        "pending_approval",
        80,
        250,
      ).catch(async (error) => {
        const latest = await client.getSession(run.session_id);
        if (latest.status === "completed") {
          return latest;
        }
        throw error;
      });

      if (view.status === "pending_approval" && view.pending) {
        waitingForApproval = true;
        setPending(view.pending);
        setPhase("awaiting_approval");
        setStatusLine(`Session ${run.session_id} · pending approval`);
        pushMessage(
          "system",
          `Guardrail paused the run. Tool **${view.pending.tool}** (${view.pending.action_kind}, ${view.pending.risk_tier} risk) needs your approval.`,
        );
        return;
      }

      if (view.status === "completed" && view.result) {
        pushMessage("assistant", view.result);
        setStatusLine(`Session ${run.session_id} · completed`);
      } else {
        pushMessage("system", `Run finished with status \`${view.status}\`.`);
        setStatusLine(`Session ${run.session_id} · ${view.status}`);
      }
    } catch (error) {
      pushMessage(
        "system",
        `Run failed: ${error instanceof Error ? error.message : String(error)}`,
      );
      setStatusLine("Run failed");
    } finally {
      if (!waitingForApproval) {
        setPhase("idle");
        setSessionId(null);
        setPending(null);
      }
    }
  };

  const finishRun = async (view: SessionView) => {
    if (view.status === "completed" && view.result) {
      pushMessage("assistant", view.result);
      setStatusLine(`Session ${view.session_id} · completed`);
      return;
    }
    if (view.status === "failed") {
      pushMessage("system", view.error ?? "Run failed after approval decision.");
      setStatusLine(`Session ${view.session_id} · failed`);
      return;
    }
    pushMessage("system", `Run ended with status \`${view.status}\`.`);
    setStatusLine(`Session ${view.session_id} · ${view.status}`);
  };

  const handleApproval = async (decision: "approve" | "deny") => {
    if (!client || !sessionId || !pending) {
      return;
    }

    setPhase("approving");
    setStatusLine(`Submitting ${decision}…`);

    try {
      await client.approve(sessionId, decision);
      setPending(null);

      if (decision === "deny") {
        pushMessage("system", "You denied the planned action. Run aborted.");
        setStatusLine(`Session ${sessionId} · denied`);
        return;
      }

      pushMessage("system", "Approved. Resuming agent loop…");
      const completed = await pollSessionUntil(
        client,
        sessionId,
        "completed",
        80,
        250,
      );
      await finishRun(completed);
    } catch (error) {
      pushMessage(
        "system",
        `Approval flow failed: ${error instanceof Error ? error.message : String(error)}`,
      );
      setStatusLine("Approval flow failed");
    } finally {
      setPhase("idle");
      setSessionId(null);
      setPending(null);
    }
  };

  const busy = phase === "running" || phase === "approving";
  const modalBusy = phase === "approving";

  return (
    <div className="flex h-full min-h-0 flex-col gap-4">
      <header className="flex flex-wrap items-center justify-between gap-3">
        <div>
          <p className="text-xs font-semibold uppercase tracking-[0.2em] text-emerald-400">
            ozr · Plan Mode
          </p>
          <h1 className="text-2xl font-semibold text-white">Workspace</h1>
        </div>
        <p className="rounded-full border border-slate-700 bg-slate-900 px-3 py-1 font-mono text-xs text-slate-300">
          {statusLine}
        </p>
      </header>

      {bootError ? (
        <div className="rounded-xl border border-red-500/40 bg-red-950/40 p-4 text-sm text-red-100">
          {bootError}
        </div>
      ) : null}

      <ChatPanel messages={messages} />

      <form
        onSubmit={(event) => {
          void handleSubmit(event);
        }}
        className="flex flex-col gap-3 rounded-2xl border border-slate-800 bg-slate-900/70 p-4 sm:flex-row"
      >
        <textarea
          value={prompt}
          onChange={(event) => setPrompt(event.target.value)}
          placeholder="Describe what ozr should do…"
          rows={3}
          disabled={busy || !!bootError || !client}
          className="min-h-[88px] flex-1 resize-y rounded-xl border border-slate-700 bg-slate-950 px-4 py-3 text-sm text-slate-100 outline-none ring-emerald-500/40 focus:ring-2 disabled:opacity-50"
        />
        <button
          type="submit"
          disabled={busy || !!bootError || !client || !prompt.trim()}
          className="rounded-xl bg-emerald-600 px-5 py-3 text-sm font-semibold text-white transition hover:bg-emerald-500 disabled:cursor-not-allowed disabled:opacity-50 sm:self-end"
        >
          {busy ? "Running…" : "Run with Guardrail"}
        </button>
      </form>

      {pending && phase === "awaiting_approval" ? (
        <ApprovalModal
          pending={pending}
          busy={modalBusy}
          onApprove={() => {
            void handleApproval("approve");
          }}
          onDeny={() => {
            void handleApproval("deny");
          }}
        />
      ) : null}
    </div>
  );
}
