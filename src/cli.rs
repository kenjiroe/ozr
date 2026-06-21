use crate::core::agent_loop::AgentLoop;
use crate::core::approval::CliApprovalGate;
use crate::core::approval_insights::{generate_approval_insights, InsightThresholds};
use crate::core::approval_report::generate_approval_dashboard;
use crate::core::audit::AuditLogger;
use crate::core::budget::BudgetGuard;
use crate::core::config::AppConfig;
use crate::core::embedding::EmbeddingSettings;
use crate::core::llm_adapter::{build_llm_provider, LlmProvider};
use crate::core::mcp_client::{build_mcp_client, McpClient};
use crate::core::memory::MemoryStore;
use crate::core::memory_orchestrator::{LayeredMemoryOrchestrator, MemoryOrchestrator, RecallBudget};
use crate::core::policy::PolicyEngine;
use crate::core::policy_pack::{BudgetPreset, PolicyPack};
use crate::core::sandbox_executor::{RuntimeExecutor, SandboxdSettings};
use crate::core::sandboxd_policy::{
    checklist_template, evaluate_production_checklist, policy_summary, render_checklist_markdown,
    CheckStatus,
};
use crate::core::replay::generate_replay_report;
use crate::core::session_recovery::{
    detect_interrupted_checkpoint, load_checkpoint, recoverable_prompt, render_status,
};
use crate::core::spec_workflow::{NoopWorkflow, SpecKittyWorkflow, WorkflowOrchestrator};
use crate::core::vector_backend::VectorBackend;
use std::env;
use std::error::Error;
use std::fs;
use std::time::Duration;

type CliResult<T> = Result<T, Box<dyn Error>>;

pub async fn run() -> CliResult<()> {
    run_async().await
}

async fn run_async() -> CliResult<()> {
    let args: Vec<String> = env::args().collect();

    match args.get(1).map(String::as_str) {
        Some("init") => init_workspace(),
        Some("config") => show_config(),
        Some("workflow") => {
            let request = args.get(2).map(String::as_str).unwrap_or("status");
            run_workflow(request)
        }
        Some("approval-report") => run_approval_report(),
        Some("approval-insights") => run_approval_insights(),
        Some("sandboxd-checklist") => run_sandboxd_checklist(),
        Some("memory") => {
            let query = args.get(2).map(String::as_str).unwrap_or("session");
            run_memory(query)
        }
        Some("replay") => {
            let run_id = args.get(2).map(String::as_str);
            run_replay(run_id)
        }
        Some("session") => {
            let action = args.get(2).map(String::as_str).unwrap_or("status");
            run_session(action).await
        }
        Some("mcp") => {
            let action = args.get(2).map(String::as_str).unwrap_or("list");
            run_mcp(action).await
        }
        Some("run") => {
            let prompt = args
                .get(2)
                .map(String::as_str)
                .unwrap_or("Summarize available tools.");
            run_prompt(prompt).await
        }
        Some("serve") | Some("api") => serve_api().await,
        _ => {
            print_help();
            Ok(())
        }
    }
}

fn init_workspace() -> CliResult<()> {
    let store = MemoryStore::new(".ozr");
    store.ensure_layout()?;
    ensure_sample_config()?;
    ensure_sandboxd_checklist_template()?;
    println!("Initialized ozr workspace at .ozr/");
    Ok(())
}

async fn serve_api() -> CliResult<()> {
    let cfg = AppConfig::from_env();
    let memory = MemoryStore::new(".ozr");
    memory.ensure_layout()?;
    println!("ozr api listening on http://{}", cfg.api_bind);
    println!("endpoints: POST /v1/run | GET /v1/session/{{id}} | POST /v1/session/{{id}}/approve | POST /v1/chat/completions");
    crate::api::serve(cfg)
        .await
        .map_err(|err| err.into())
}

async fn run_prompt(prompt: &str) -> CliResult<()> {
    let cfg = AppConfig::from_env();

    let memory = MemoryStore::new(".ozr");
    memory.ensure_layout()?;

    if let Some(interrupted) = detect_interrupted_checkpoint()? {
        eprintln!(
            "Detected interrupted session {} (last_event={}). Use `ozr session resume` to continue.",
            interrupted.run_id, interrupted.last_event
        );
    }

    let mut audit = AuditLogger::new(".ozr/audit/runs.log")?;
    let mut policy = PolicyEngine::default();
    policy.ponytail_mode = cfg.ponytail_mode;

    let policy_pack = PolicyPack::from_env(&cfg.policy_pack);
    let mut budget_preset = BudgetPreset {
        max_tokens: cfg.budget_max_tokens,
        max_iterations: cfg.budget_max_iterations,
        max_run_seconds: cfg.budget_max_run_seconds,
    };
    policy_pack.apply(&mut policy, &cfg.approval_mode, &mut budget_preset);
    let _ = audit.append(
        "bootstrap",
        &format!("policy_pack:{}", policy_pack.as_str()),
    );

    if cfg.feature_spec_kitty_workflow {
        let workflow = SpecKittyWorkflow::new(cfg.spec_kitty_command.clone());
        let workflow_result = workflow.dispatch(prompt);
        match workflow_result {
            Ok(output) => {
                let _ = audit.append("bootstrap", &format!("spec_kitty_dispatch_ok:{}", output));
            }
            Err(err) => {
                let _ = audit.append("bootstrap", &format!("spec_kitty_dispatch_err:{}", err));
            }
        }
    }

    let mut enriched_prompt = prompt.to_string();
    if cfg.feature_memory_layered {
        let orchestrator = build_memory_orchestrator(&cfg)?;
        let budget = memory_recall_budget(&cfg);
        let bundle = orchestrator.recall(prompt, budget)?;
        let _ = orchestrator.ingest_event(&format!("prompt={}", prompt));
        let memory_hits = orchestrator.format_bundle(&bundle);
        enriched_prompt = format!("{}\n\nMemory context:\n{}", prompt, memory_hits);
    }

    let budget = BudgetGuard::new(
        budget_preset.max_tokens,
        budget_preset.max_iterations,
        Duration::from_secs(budget_preset.max_run_seconds),
    );
    let llm = build_llm_provider(&cfg);
    let mcp = build_mcp_client(&cfg);
    run_with_provider(
        cfg,
        enriched_prompt,
        memory,
        budget,
        llm,
        mcp,
        &mut audit,
        policy,
    )
    .await
}

async fn run_with_provider(
    cfg: AppConfig,
    enriched_prompt: String,
    memory: MemoryStore,
    budget: BudgetGuard,
    llm: Box<dyn LlmProvider>,
    mcp: Box<dyn McpClient>,
    audit: &mut AuditLogger,
    policy: PolicyEngine,
) -> CliResult<()> {
    let final_answer = {
        let executor = RuntimeExecutor::from_config(&cfg);
        let approver = CliApprovalGate::new(cfg.approval_mode);
        let mut loop_engine =
            AgentLoop::new(policy, budget, llm, mcp, executor, approver, memory, audit);
        loop_engine.run_once(&enriched_prompt).await?
    };

    if cfg.feature_memory_layered {
        let orchestrator = build_memory_orchestrator(&cfg)?;
        let summary = final_answer.chars().take(240).collect::<String>();
        let _ = orchestrator.ingest_event(&format!("run_completed:summary={}", summary));
    }

    println!("{}", final_answer);
    Ok(())
}

async fn run_mcp(action: &str) -> CliResult<()> {
    let cfg = AppConfig::from_env();
    let client = build_mcp_client(&cfg);
    match action {
        "list" => {
            for tool in client.list_tool_definitions().await {
                println!(
                    "- {} ({:?}) {}",
                    tool.name, tool.action_kind, tool.description
                );
            }
            Ok(())
        }
        "call" => Err("usage: ozr mcp list (call via agent run)".into()),
        _ => Err(format!("unknown mcp action: {} (use list)", action).into()),
    }
}

fn show_config() -> CliResult<()> {
    let cfg = AppConfig::from_env();
    println!("ozr config");
    println!(
        "- OZR_FEATURE_SPEC_KITTY_WORKFLOW={} ",
        cfg.feature_spec_kitty_workflow
    );
    println!(
        "- OZR_FEATURE_SANDBOXD_EXECUTOR={} ",
        cfg.feature_sandboxd_executor
    );
    println!("- OZR_FEATURE_MEMORY_LAYERED={} ", cfg.feature_memory_layered);
    println!("- OZR_FEATURE_VECTOR_BACKEND={} ", cfg.feature_vector_backend);
    println!(
        "- OZR_FEATURE_PONYTAIL_PROFILE={} ",
        cfg.ponytail_mode.as_str()
    );
    println!("- OZR_APPROVAL_MODE={} ", cfg.approval_mode.as_str());
    println!("- OZR_LLM_BACKEND={} ", cfg.llm_backend);
    println!("- OZR_LLM_API_URL={} ", cfg.llm_api_url);
    println!("- OZR_LLM_API_KEY_SET={} ", !cfg.llm_api_key.trim().is_empty());
    println!("- OZR_LLM_MODEL={} ", cfg.llm_model);
    println!("- OZR_MCP_BACKEND={} ", cfg.mcp_backend);
    println!("- OZR_MCP_STDIO_COMMAND={} ", cfg.mcp_stdio_command);
    println!("- OZR_MCP_STDIO_ARGS={} ", cfg.mcp_stdio_args);
    println!("- OZR_MCP_STDIO_TIMEOUT_MS={} ", cfg.mcp_stdio_timeout_ms);
    println!(
        "- OZR_MCP_STDIO_RETRY_ATTEMPTS={} ",
        cfg.mcp_stdio_retry_attempts
    );
    println!("- OZR_MCP_STDIO_FRAMING={} ", cfg.mcp_stdio_framing);
    println!("- OZR_SPEC_KITTY_COMMAND={} ", cfg.spec_kitty_command);
    println!("- OZR_SANDBOXD_API_BASE={} ", cfg.sandboxd_api_base);
    println!(
        "- OZR_SANDBOXD_API_TOKEN_SET={} ",
        !cfg.sandboxd_api_token.trim().is_empty()
    );
    println!("- OZR_SANDBOXD_SANDBOX_ID={} ", cfg.sandboxd_sandbox_id);
    println!("- OZR_SANDBOXD_AGENT={} ", cfg.sandboxd_agent);
    println!("- OZR_SANDBOXD_POLL_ATTEMPTS={} ", cfg.sandboxd_poll_attempts);
    println!(
        "- OZR_SANDBOXD_POLL_INTERVAL_MS={} ",
        cfg.sandboxd_poll_interval_ms
    );
    println!(
        "- OZR_SANDBOXD_POLL_BACKOFF_MULTIPLIER={} ",
        cfg.sandboxd_poll_backoff_multiplier
    );
    println!(
        "- OZR_SANDBOXD_POLL_MAX_INTERVAL_MS={} ",
        cfg.sandboxd_poll_max_interval_ms
    );
    println!(
        "- OZR_SANDBOXD_CAPTURE_EVENTS={} ",
        cfg.sandboxd_capture_events
    );
    println!(
        "- OZR_SANDBOXD_EVENTS_MAX_TIME_S={} ",
        cfg.sandboxd_events_max_time_s
    );
    println!("- OZR_SANDBOXD_REQUIRE_AUTH={} ", cfg.sandboxd_require_auth);
    println!("- OZR_SANDBOXD_HTTPS_ONLY={} ", cfg.sandboxd_https_only);
    println!("- OZR_MEMORY_RECALL_LIMIT={} ", cfg.memory_recall_limit);
    println!("- OZR_MEMORY_BACKEND={} ", cfg.memory_backend);
    println!("- OZR_MEMORY_TRUST_THRESHOLD={} ", cfg.memory_trust_threshold);
    println!(
        "- OZR_MEMORY_RECALL_TOKEN_BUDGET={} ",
        cfg.memory_recall_token_budget
    );
    println!("- OZR_QDRANT_URL={} ", cfg.qdrant_url);
    println!("- OZR_QDRANT_COLLECTION={} ", cfg.qdrant_collection);
    println!(
        "- OZR_QDRANT_API_KEY_SET={} ",
        !cfg.qdrant_api_key.trim().is_empty()
    );
    println!("- OZR_VECTOR_EMBEDDINGS={} ", cfg.vector_embeddings);
    println!("- OZR_EMBEDDING_API_URL={} ", cfg.embedding_api_url);
    println!(
        "- OZR_EMBEDDING_API_KEY_SET={} ",
        !embedding_api_key(&cfg).trim().is_empty()
    );
    println!("- OZR_EMBEDDING_MODEL={} ", cfg.embedding_model);
    println!("- OZR_EMBEDDING_DIMENSIONS={} ", cfg.embedding_dimensions);
    println!(
        "- OZR_APPROVAL_ALERT_DENIAL_RATE={} ",
        cfg.approval_alert_denial_rate
    );
    println!(
        "- OZR_APPROVAL_ALERT_RETRY_RATE={} ",
        cfg.approval_alert_retry_rate
    );
    println!(
        "- OZR_APPROVAL_ALERT_HIGH_RISK_SHARE={} ",
        cfg.approval_alert_high_risk_share
    );
    println!("- OZR_BUDGET_MAX_TOKENS={} ", cfg.budget_max_tokens);
    println!("- OZR_BUDGET_MAX_ITERATIONS={} ", cfg.budget_max_iterations);
    println!("- OZR_BUDGET_MAX_RUN_SECONDS={} ", cfg.budget_max_run_seconds);
    println!("- OZR_POLICY_PACK={} ", cfg.policy_pack);
    println!("- OZR_API_BIND={} ", cfg.api_bind);
    Ok(())
}

fn run_workflow(request: &str) -> CliResult<()> {
    let cfg = AppConfig::from_env();
    let result = if cfg.feature_spec_kitty_workflow {
        let workflow = SpecKittyWorkflow::new(cfg.spec_kitty_command);
        workflow.dispatch(request)
    } else {
        let workflow = NoopWorkflow::default();
        workflow.dispatch(request)
    }?;

    println!("{}", result);
    Ok(())
}

fn run_memory(query: &str) -> CliResult<()> {
    let cfg = AppConfig::from_env();
    let store = MemoryStore::new(".ozr");
    store.ensure_layout()?;

    let orchestrator = build_memory_orchestrator(&cfg)?;
    let budget = memory_recall_budget(&cfg);
    let bundle = orchestrator.recall(query, budget)?;
    let score = orchestrator.score(&bundle, query);
    let result = orchestrator.format_bundle(&bundle);
    println!("recall_score={:.2}", score);
    println!("{}", result);
    Ok(())
}

fn run_sandboxd_checklist() -> CliResult<()> {
    let cfg = AppConfig::from_env();
    let settings = sandboxd_settings_from_config(&cfg);
    let items = evaluate_production_checklist(cfg.feature_sandboxd_executor, &settings);
    let policy = policy_summary(&settings);
    let report = render_checklist_markdown(&items, &policy);
    fs::create_dir_all(".ozr/audit")?;
    fs::write(".ozr/audit/sandboxd-checklist.md", &report)?;
    let fail_count = items
        .iter()
        .filter(|item| item.status == CheckStatus::Fail)
        .count();
    println!("sandboxd checklist written: .ozr/audit/sandboxd-checklist.md");
    println!("{}", report);
    if fail_count > 0 {
        Err(format!("sandboxd checklist has {} failing checks", fail_count).into())
    } else {
        Ok(())
    }
}

fn run_approval_insights() -> CliResult<()> {
    let cfg = AppConfig::from_env();
    let thresholds = InsightThresholds {
        denial_rate: cfg.approval_alert_denial_rate,
        retry_rate: cfg.approval_alert_retry_rate,
        high_risk_share: cfg.approval_alert_high_risk_share,
        tool_denial_min: 2,
    };
    let message = generate_approval_insights(
        ".ozr/audit/runs.log",
        ".ozr/audit/approval-insights.md",
        &thresholds,
    )?;
    println!("{}", message);
    Ok(())
}

fn run_approval_report() -> CliResult<()> {
    let message = generate_approval_dashboard(
        ".ozr/audit/runs.log",
        ".ozr/audit/approval-dashboard.md",
    )?;
    println!("{}", message);
    Ok(())
}

fn run_replay(run_id: Option<&str>) -> CliResult<()> {
    let output = match run_id {
        Some(id) => format!(".ozr/audit/replay-{}.md", id),
        None => ".ozr/audit/replay-latest.md".to_string(),
    };
    let message = generate_replay_report(".ozr/audit/runs.log", run_id, ".ozr/audit", &output)?;
    println!("{}", message);
    if message.starts_with("no audit runs") {
        return Ok(());
    }
    let report = fs::read_to_string(&output)?;
    println!("{}", report);
    Ok(())
}

async fn run_session(action: &str) -> CliResult<()> {
    match action {
        "status" => {
            if let Some(checkpoint) = load_checkpoint()? {
                println!("{}", render_status(&checkpoint));
            } else {
                println!("no session checkpoint found");
            }
            Ok(())
        }
        "resume" => {
            let prompt = recoverable_prompt()?
                .ok_or("no recoverable session checkpoint (need interrupted or failed status)")?;
            println!("Resuming previous prompt...");
            run_prompt(&prompt).await
        }
        _ => Err(format!("unknown session action: {} (use status|resume)", action).into()),
    }
}

fn print_help() {
    println!("ozr - AI Agent Harness (Rust MVP)");
    println!();
    println!("Usage:");
    println!("  ozr init");
    println!("  ozr config");
    println!("  ozr workflow \"<request>\"");
    println!("  ozr approval-report");
    println!("  ozr approval-insights");
    println!("  ozr sandboxd-checklist");
    println!("  ozr memory \"<query>\"");
    println!("  ozr mcp list");
    println!("  ozr replay [run_id]");
    println!("  ozr session status|resume");
    println!("  ozr run \"<prompt>\"");
    println!("  ozr serve");
    println!();
    println!("Approval mode via env: OZR_APPROVAL_MODE=prompt|auto|deny");
    println!("Prompt approvals support: approve | deny | skip | retry | edit");
    println!("LLM backend via config/env: OZR_LLM_BACKEND=mock|openai-compatible|anthropic|gemini|ollama");
    println!("MCP backend via config/env: OZR_MCP_BACKEND=mock|stdio");
    println!("MCP stdio framing: OZR_MCP_STDIO_FRAMING=ndjson|content-length");
}

fn ensure_sandboxd_checklist_template() -> CliResult<()> {
    let path = ".ozr/sandboxd-production-checklist.md";
    if std::path::Path::new(path).exists() {
        return Ok(());
    }
    fs::write(path, checklist_template())?;
    Ok(())
}

fn sandboxd_settings_from_config(cfg: &AppConfig) -> SandboxdSettings {
    SandboxdSettings::from_config(cfg)
}

fn ensure_sample_config() -> CliResult<()> {
    let config_path = ".ozr/config.env";
    if std::path::Path::new(config_path).exists() {
        return Ok(());
    }

    let sample = "# ozr runtime config\nOZR_LLM_BACKEND=mock\nOZR_LLM_API_URL=https://api.openai.com/v1/chat/completions\nOZR_LLM_API_KEY=\nOZR_LLM_MODEL=gpt-4o-mini\nOZR_MCP_BACKEND=mock\nOZR_MCP_STDIO_COMMAND=\nOZR_MCP_STDIO_ARGS=\nOZR_MCP_STDIO_TIMEOUT_MS=5000\nOZR_MCP_STDIO_RETRY_ATTEMPTS=2\nOZR_MCP_STDIO_FRAMING=ndjson\n# Real filesystem MCP (after: npm install --prefix tests/fixtures/mcp-filesystem)\n# OZR_MCP_BACKEND=stdio\n# OZR_MCP_STDIO_COMMAND=node\n# OZR_MCP_STDIO_ARGS=tests/fixtures/mcp-filesystem/node_modules/@modelcontextprotocol/server-filesystem/dist/index.js tests/fixtures/mcp_fs_root\nOZR_FEATURE_SPEC_KITTY_WORKFLOW=false\nOZR_FEATURE_SANDBOXD_EXECUTOR=false\nOZR_SANDBOXD_REQUIRE_AUTH=false\nOZR_SANDBOXD_HTTPS_ONLY=false\nOZR_FEATURE_MEMORY_LAYERED=false\nOZR_FEATURE_VECTOR_BACKEND=none\nOZR_VECTOR_EMBEDDINGS=false\nOZR_EMBEDDING_API_URL=https://api.openai.com/v1/embeddings\nOZR_EMBEDDING_API_KEY=\nOZR_EMBEDDING_MODEL=text-embedding-3-small\nOZR_EMBEDDING_DIMENSIONS=1536\nOZR_QDRANT_URL=http://127.0.0.1:6333\nOZR_QDRANT_COLLECTION=ozr_memory\nOZR_QDRANT_API_KEY=\nOZR_MEMORY_BACKEND=sqlite\nOZR_MEMORY_TRUST_THRESHOLD=0.5\nOZR_MEMORY_RECALL_LIMIT=3\nOZR_MEMORY_RECALL_TOKEN_BUDGET=500\nOZR_BUDGET_MAX_TOKENS=2000\nOZR_BUDGET_MAX_ITERATIONS=5\nOZR_BUDGET_MAX_RUN_SECONDS=15\nOZR_POLICY_PACK=balanced\nOZR_API_BIND=127.0.0.1:8080\nOZR_APPROVAL_ALERT_DENIAL_RATE=0.3\nOZR_APPROVAL_ALERT_RETRY_RATE=0.2\nOZR_APPROVAL_ALERT_HIGH_RISK_SHARE=0.4\nOZR_FEATURE_PONYTAIL_PROFILE=off\nOZR_APPROVAL_MODE=prompt\n";
    fs::write(config_path, sample)?;
    Ok(())
}


fn build_memory_orchestrator(cfg: &AppConfig) -> CliResult<LayeredMemoryOrchestrator> {
    let store = MemoryStore::new(".ozr");
    let backend = if cfg.feature_memory_layered {
        cfg.memory_backend.as_str()
    } else {
        "file"
    };
    let vector = VectorBackend::from_config(
        &cfg.feature_vector_backend,
        &cfg.qdrant_url,
        &cfg.qdrant_collection,
        &cfg.qdrant_api_key,
        build_embedding_settings(cfg),
    );
    LayeredMemoryOrchestrator::from_config(store, backend, cfg.memory_trust_threshold, vector)
        .map_err(|e| e.into())
}

fn build_embedding_settings(cfg: &AppConfig) -> Option<EmbeddingSettings> {
    if !cfg.vector_embeddings || cfg.feature_vector_backend != "qdrant" {
        return None;
    }
    let api_key = embedding_api_key(cfg);
    Some(EmbeddingSettings {
        api_url: cfg.embedding_api_url.clone(),
        api_key,
        model: cfg.embedding_model.clone(),
        dimensions: cfg.embedding_dimensions,
    })
}

fn embedding_api_key(cfg: &AppConfig) -> String {
    if !cfg.embedding_api_key.trim().is_empty() {
        cfg.embedding_api_key.clone()
    } else {
        cfg.llm_api_key.clone()
    }
}

fn memory_recall_budget(cfg: &AppConfig) -> RecallBudget {
    RecallBudget {
        max_items: cfg.memory_recall_limit,
        max_tokens: cfg.memory_recall_token_budget,
    }
}