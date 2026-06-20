import ReactMarkdown from "react-markdown";

export interface ChatEntry {
  id: string;
  role: "user" | "assistant" | "system";
  content: string;
}

interface ChatPanelProps {
  messages: ChatEntry[];
}

function roleLabel(role: ChatEntry["role"]) {
  switch (role) {
    case "user":
      return "You";
    case "assistant":
      return "ozr";
    default:
      return "System";
  }
}

export function ChatPanel({ messages }: ChatPanelProps) {
  return (
    <div className="flex min-h-0 flex-1 flex-col gap-3 overflow-y-auto rounded-2xl border border-slate-800 bg-slate-950/70 p-4">
      {messages.length === 0 ? (
        <p className="text-sm text-slate-500">
          Send a prompt to start a guarded agent run. Try{" "}
          <code className="rounded bg-slate-900 px-1.5 py-0.5 text-xs text-slate-300">
            run mystery shell task
          </code>{" "}
          to trigger the approval modal.
        </p>
      ) : (
        messages.map((message) => (
          <article
            key={message.id}
            className={`rounded-xl border px-4 py-3 ${
              message.role === "user"
                ? "border-sky-500/30 bg-sky-950/30"
                : message.role === "assistant"
                  ? "border-emerald-500/20 bg-emerald-950/20"
                  : "border-slate-700 bg-slate-900/60"
            }`}
          >
            <header className="mb-2 text-xs font-semibold uppercase tracking-wide text-slate-400">
              {roleLabel(message.role)}
            </header>
            <div className="prose prose-invert prose-sm max-w-none text-slate-100">
              <ReactMarkdown>{message.content}</ReactMarkdown>
            </div>
          </article>
        ))
      )}
    </div>
  );
}
