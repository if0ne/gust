# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

Gust is a single-node workflow orchestrator where **each task is a WebAssembly component** (WASI Preview 2) run under wasmtime. A workflow (a DAG of tasks) is authored in YAML, **triggered manually via the API** (there is no cron scheduler — see "manual execution only" below), and executed against a PostgreSQL-backed state store. A React UI is embedded into the binary and served from the same HTTP server. Rust edition 2024.

## Commands

The store has two backends, selected at startup by `DATABASE_URL` (see `infra::store::Store`): a durable Postgres backend, and an ephemeral in-memory backend used when `DATABASE_URL` is unset/empty or set to `memory`. **For local dev, run with no `DATABASE_URL` and the app needs no infrastructure at all** (no Postgres, no migrations) — all state lives in process and is lost on exit. The repo uses sqlx **runtime** queries (`sqlx::query`/`query_as`), not the compile-time `query!` macro — so no database connection is needed at compile time either.

```powershell
# Zero-infrastructure local dev (in-memory store, no DATABASE_URL):
$env:SKIP_WEB_BUILD = "1"; cargo run   # executor loop + HTTP server on :8080

# Against Postgres (durable). Migrations in migrations/ run automatically on startup:
docker compose up -d postgres          # local Postgres (gust/gust@localhost:5432/gust)
$env:DATABASE_URL = "postgres://gust:gust@localhost:5432/gust"
cargo run

cargo build                            # also builds the web UI via build.rs (needs npm)
$env:SKIP_WEB_BUILD = "1"; cargo build # skip the frontend build for fast backend iteration
cargo check
cargo clippy
cargo fmt
```

There are **no Rust tests** in this repo yet (`cargo test` is a no-op). Verification is done by running the app (either backend) and exercising the API / UI.

Frontend (lives in `web/`, only needed when changing the UI):

```powershell
cd web
npm install
npm run dev      # Vite dev server, proxies /api -> localhost:8080
npm run build    # emits web/dist, which the Rust binary embeds
```

Full container build (multi-stage: Node builds UI → Rust builds binary → slim runtime):

```powershell
docker compose up --build
```

### build.rs / web embedding

`build.rs` runs `npm install && npm run build` in `web/` and writes `web/dist`, which is embedded at compile time via `rust-embed-for-web` (`WebAssets` in `src/handler/workflow.rs`). It degrades gracefully: if `SKIP_WEB_BUILD=1`, npm is missing, or the build fails, it writes a placeholder `web/dist/index.html` instead of panicking. Note `rust-embed-for-web` reads assets **from disk at request time in debug builds** and **embeds them in release builds** — so a debug binary needs `web/dist` to exist at runtime.

## Code conventions

Match the surrounding code. In particular:

- **`anyhow::Result`** — refer to it by its fully-qualified path (`anyhow::Result<T>`); do **not** `use anyhow::Result;`. This keeps it visually distinct from `std::result::Result` / `sqlx::Result` at call sites.
- **Locks use `parking_lot`** — for in-memory synchronization use `parking_lot::Mutex` / `parking_lot::RwLock`, not the `std::sync` equivalents. They don't poison, so `.lock()` / `.write()` return the guard directly with no `.unwrap()`.
- **Blank line before the final expression** — when a function or block ends in its returned value (a trailing expression or `return`, e.g. `Ok(rows)`), leave one empty line before it to separate the result from the work above.
- **No `mod.rs`** — never create `mod.rs`. Declare a module with submodules as a sibling file next to a same-named folder: `foo.rs` + `foo/` (e.g. `service/runtime.rs` + `service/runtime/`), not `foo/mod.rs`. (The lone existing `job/executor/mod.rs` is legacy; new modules follow the `foo.rs` pattern, and prefer migrating old ones when you touch them.)

## Architecture

The codebase is split into layers: `handler/` (HTTP), `service/` (domain: `workflow` + `runtime`), `infra/store/` (persistence), and `job/` (background work + run materialization). `main.rs` wires everything and spawns **one** background loop (the executor) plus the HTTP server, all sharing an `Arc<dyn Store>`. `Store` is a **trait** (`infra/store.rs`), composed from per-entity supertraits `WorkflowStore + WorkflowRunStore + TaskStore` (one per file: `infra/store/{workflow,workflow_run,task_instance}`), plus `ping`. Two impls: `PostgresStore` (sqlx) and `MemoryStore` (`Mutex<MemData>` of in-process `HashMap`s). When adding a store operation, declare it on the relevant entity trait and implement it for **both** structs. Note: callers hold `Arc<dyn Store>`, so the trait methods are callable without importing the supertraits (the `dyn Store` type already names them). There is no in-process message passing between components — **the `task_instance` table is the work queue and the source of truth**, and all coordination happens through its row states.

`MemoryStore` uses a single `Mutex` over all tables so cross-table operations stay atomic; its methods never `.await` while holding the lock (keeping the spawned task futures `Send`). The Postgres-specific concurrency primitive — the SKIP-LOCKED claim — lives in `TaskStore::claim_and_mark_running`, which the executor calls instead of issuing SQL directly.

### Manual execution only

There is **no cron scheduler** — a workflow runs only when triggered via `POST /api/workflows/{id}/trigger`. That handler creates a `workflow_run` row (`create_idempotent`) and then `job::materialize_tasks` inserts the run's `task_instance` rows (roots with no `depends_on` start `queued`, the rest `pending`). `WorkflowSpec` has no schedule/catchup fields. (The store still has `list_active`/`max_logical_date`, unused now — leftovers from the removed scheduler.)

### The executor loop + the API

- **Executor** (`job/executor/mod.rs`, `Executor` struct holding `Arc<dyn Store>`; loop ticks every 500ms): claims up to n `queued` tasks (n = free semaphore permits) via `TaskStore::claim_and_mark_running` and runs each on a tokio task gated by a concurrency `Semaphore` (`EXECUTOR_MAX_CONCURRENCY`). On Postgres that claim is a `FOR UPDATE SKIP LOCKED` transaction — the pattern that would make the executor safe to run as multiple workers.
- **API** (handlers in `handler/{workflow,status}.rs`; the router is built in `main.rs`): endpoints under `/api/*` for workflows, runs, task instances, and logs, plus `/healthz` (liveness, always 200) and `/readyz` (readiness — `Store::ping`, 503 if the backend is unreachable). Everything else falls through to `serve_ui`, which serves the embedded SPA (with ETag/304 revalidation and gzip negotiation), falling back to `index.html` for client-side routes.

### Task state machine

`task_instance.state` drives execution. Transitions are computed by `WorkflowSpec` methods (`ready_tasks` / `upstream_failed_tasks` in `service/workflow/spec.rs`) plus `service/workflow/graph::all_terminal`, and applied by the executor's `advance_graph` after every task finishes:

```
pending ──(all deps success)──> queued ──(claimed)──> running ──> success
   │                                                          └──> failed ──(try_number < max_retries)──> queued (retry)
   └──(any dep failed/upstream_failed)──> upstream_failed
```

After each task completes, `advance_graph` re-reads the full state map for the run, promotes newly-ready `pending`→`queued`, propagates `upstream_failed`, and — when **all** tasks are terminal (`success`/`failed`/`upstream_failed`/`skipped`) — finalizes the `workflow_run` as `success` (all succeeded) or `failed`. On retry the graph is intentionally **not** advanced, so the task gets re-claimed. (Note: `WorkflowSpec` no longer carries a `retries` field, so `materialize_tasks` sets `max_retries = 0` and the retry branch is currently inert.)

### WASM runtime (`service/runtime/`)

Runs a component's `wasi:cli/run` export under wasmtime 45 (component model, async). The runtime is built around **one shared `Engine`** (`engine.rs`, a thin wrapper over `wasmtime::Engine`) reused across every workflow, so compilation and the epoch-ticker amortize globally. Compiled `Component`s are memoized in a moka cache keyed by content digest (`cache.rs`), and the engine auto-detects at startup whether to enable the pooling allocator.

`Runner` (`runner.rs`) is a **registry**, not a per-call object: it holds the `Engine` plus `workloads: RwLock<HashMap<workload_id, Workload>>`, where a `Workload` maps `component_id` (task id) → `ResolvedComponent`. The flow is:
- `load_component(workload_id, component_id, ComponentDesc)` — compiles + resolves the component and stores it under `(workload_id, component_id)`. Idempotent; compilation is digest-cached so reloads are cheap. The executor calls this with `workload_id = workflow_id` and `component_id = task_id`.
- `run(workload_id, component_id)` — clones the `Arc`-backed `ResolvedComponent` out of the lock and instantiates it, so no lock is held during execution.

A `ResolvedComponent` (`resolved.rs`) is produced in two steps: `UnresolvedComponent::new` (`component.rs`) compiles the bytes and extracts the component's WIT world (imports/exports, modeled in `wit.rs`) from its `component_type()`; `UnresolvedComponent::resolve(extensions)` then binds extensions, freezes the linker, and yields an `Arc`-backed, cloneable `ResolvedComponent`. The modules are split to stay acyclic: `component.rs` is extension-unaware (only compiles + describes the WIT world), `resolved.rs` holds the resolved type (depends on `extension` but never on `component`), and `binding.rs` is the one module depending on both — it hosts the `resolve` method (an inherent `impl UnresolvedComponent` block defined there, not in `component.rs`) plus the `UnresolvedComponent → ResolvedComponent` transform. Instantiation goes through `instantiate_pre()` → `CommandPre` so the pre-linked component can be reused. The store data is `Context` (`ctx.rs`), which implements both `WasiView` and `WasiHttpView`.

`Runner` is constructed via `RunnerBuilder` (it builds a fresh `Engine` and collects the registered extensions) and has `start()`/`stop()` lifecycle methods that fan out to each registered `Extension` (`extension.rs` — a trait with `configure_ctx` plus lifecycle hooks for extending the runtime; **scaffolding, no implementors wired in yet**). `main.rs` builds the runner and calls `start()` before spawning the executor, and `stop()` on shutdown.

Per-task isolation: stdout/stderr captured via `MemoryOutputPipe` (capped at 4 MiB each, persisted to `task_log`), and timeout enforced via **epoch interruption** — a tokio `EpochTicker` task increments the engine epoch every 10ms, and `Engine::new_store` sets the deadline to `DEFAULT_EPOCH_DEADLINE_SECS` (30s) worth of ticks. The timeout is fixed/uniform (engine-owned; the spec has no per-task timeout). Memory is bounded by the pooling allocator rather than per-store `StoreLimits`.

**wasmtime 45 API specifics** (differ from older versions — don't "fix" these to old signatures):
- `WasiView::ctx()` returns `WasiCtxView<'_>` (a `{ ctx, table }` struct), not `&mut WasiCtx`; `WasiHttpView::http()` likewise returns a `WasiHttpCtxView<'_>`.
- `ResourceTable` comes from `wasmtime::component`.
- `Command`/`CommandPre` bindings + `add_to_linker_async` + `MemoryOutputPipe` live under `wasmtime_wasi::p2`.
- instantiate via `CommandPre::new(instance_pre)?` then `command_pre.instantiate_async(&mut store)` — the runtime pre-links, so it does **not** call `Command::instantiate_async` directly.
- async support is always on; `Config::async_support` is deprecated/no-op.

### Component resolution (`service/runtime/resolver.rs`, `service/runtime/image.rs`)

A task's component is an `ImageRef` (`service/runtime/image.rs`) — a single string parsed by scheme via `FromStr`/`Display`:
- bare/relative path (e.g. `./foo.wasm`) → `RelativePath`, resolved against `WORKFLOW_BASE_DIR` (project root under `cargo run`, CWD in production)
- `file://…` → `AbsolutePath`
- `data://<base64>` → inline `Base64` bytes
- `http://…` / `https://…` → `Oci`, pulled via `oci-wasm`

`Resolver` is a trait (`async fn resolve(&ImageRef) -> ResolvedImage { bytes, digest }`); `DefaultResolver` is the impl, wired in `main.rs` as `Arc<dyn Resolver>` and held by the `Executor`. `digest` is a sha256 of the bytes (base64/sha2) and is used as the engine's compiled-component cache key. `DefaultResolver.cache_dir` is currently unused (no on-disk OCI cache yet).

### Workflow spec & validation (`service/workflow/`)

The `POST /api/workflows` API accepts a **JSON** `WorkflowSpec` body — `{ id, max_active_tasks?, tasks: [{ id, component: { name, image, extensions? }, depends_on? }] }` (`serde_json` → `WorkflowSpec`; `component` is a `ComponentSpec` from `service/runtime/types.rs`). Authoring is in YAML, but the conversion happens in the frontend (`web/src/api.ts` uses `js-yaml`) — the server only speaks JSON. Validation is `WorkflowSpec::validate()` (in `spec.rs`), which builds a petgraph `DiGraph`, rejects duplicate/empty IDs and unknown `depends_on` targets, and runs `toposort` to detect cycles. On create, the validated spec is re-serialized to canonical YAML (stored as `workflow.yaml_source`, shown in the UI's workflow detail) and also stored as JSONB in `workflow.spec`; the executor and `materialize_tasks` deserialize the JSONB back into a `WorkflowSpec`. See `examples/workflows/hello_world.yaml` for the shape and `examples/components/hello/` for a minimal component.

## Layout

- `src/config.rs` — all config from env vars (`DATABASE_URL` selects the backend; the rest, with defaults: `WORKFLOW_BASE_DIR`, `BIND_ADDR`, `EXECUTOR_MAX_CONCURRENCY`, `COMPONENT_CACHE_DIR`). Execution limits (timeout/memory) are no longer config — they live on the shared `Engine`
- `src/infra/store.rs` + `src/infra/store/` — the `Store` trait + `PostgresStore`/`MemoryStore` impls (`store.rs`), plus per-entity supertraits split by table: `workflow` (`WorkflowStore`), `workflow_run` (`WorkflowRunStore`), `task_instance` (`TaskStore`). Each trait is implemented for both backends (raw sqlx for Postgres, no ORM)
- `src/service/workflow/` — `spec` (serde types + validation + ready/upstream-failed task selection) and `graph` (`all_terminal`). A `TaskSpec.component` is a `ComponentSpec` from `service/runtime/common.rs`
- `src/service/runtime/` — the WASM runtime: `engine` (shared `Engine` + `EpochTicker` + pooling-allocator detection), `cache` (moka compiled-component cache), `runner` (`Runner` registry + `RunnerBuilder`), `component` (`UnresolvedComponent` — compilation + WIT-world extraction, extension-unaware), `resolved` (`ResolvedComponent` — the bound, instantiation-ready component; depends on `extension`, not `component`), `binding` (the `UnresolvedComponent::resolve` transform; the only module depending on both `component` and `extension`), `ctx` (`Context`/`ContextBuilder` — `WasiView` + `WasiHttpView`), `types` (`ComponentId`/`ExtensionId`/`ComponentSpec`/`ComponentDesc`/`InterfaceConfigMap`), `image` (`ImageRef`), `wit` (`WitInterface`/`WitWorld`), `extension` (`Extension` trait — scaffolding), `resolver` (`Resolver` trait + `DefaultResolver`)
- `src/job/` — `executor` (the run loop) and `materialize_tasks` (creates a run's task rows on trigger)
- `src/handler/` — HTTP handlers: `workflow` (REST + embedded UI), `status` (health probes); the router itself is built in `main.rs`
- `migrations/001_initial.sql` — schema: `workflow`, `workflow_run`, `task_instance`, `task_log`
- `web/` — TypeScript + React + Vite SPA (react-router); `web/dist` is the embed source

## Out of scope (MVP)

Cron scheduling (workflows are manual-trigger only), XCom/inter-task data passing, multi-node coordination, and auth are intentionally not implemented. An extension/plugin system is **scaffolded** (`service/runtime/extension.rs`, plus `RunnerBuilder::extension` and the `Runner` start/stop lifecycle) but has no implementors wired in yet.
