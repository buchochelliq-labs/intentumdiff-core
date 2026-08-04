//! The native IntentumDiff CLI (`intentumdiff`) — #B.4b. A thin clap front-end over the pure-Rust
//! engine: it links `intentumdiff-rust-core` (no pyo3) and drives the ungated engine `*_impl`
//! functions + the ungated cache/analytics stores in-process. This replaces the Python
//! `intentumdiff` console script; commands are added slice by slice (cache first).

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use intentumdiff_rust_core::analytics_store::AnalyticsStore;
use intentumdiff_rust_core::cache_store::{SqliteStore, StoreError};
use serde_json::json;

/// The version/build descriptor shown by `--version` (clap prepends the bin name) so this native
/// binary is unmistakable from the Python `intentumdiff` console script (which resolves first on
/// PATH). `intentumdiff engine` prints the same with the name.
const ENGINE_BANNER: &str = concat!(
    env!("CARGO_PKG_VERSION"),
    " — NATIVE build (pure-Rust core, no Python / pyo3)"
);

#[derive(Parser)]
#[command(
    name = "intentumdiff",
    version = ENGINE_BANNER,
    about = "IntentumDiff — semantic diff engine · NATIVE clap CLI (pure-Rust core, no Python runtime)",
    long_about = "IntentumDiff — semantic diff engine.\n\nThis is the NATIVE clap CLI: it links the pure-Rust core directly (no pyo3, no \
                  Python runtime). If a bare `intentumdiff` invocation looks different, PATH is \
                  resolving the Python console script instead — run this binary by its full path, \
                  or check `intentumdiff engine`."
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Print the running engine banner (native Rust core) — the Python CLI has no such command,
    /// so `intentumdiff engine` is the quickest way to tell the two apart.
    Engine,
    /// Diff two local files (semantic diff via the native engine).
    File {
        /// Old (left) file.
        old: PathBuf,
        /// New (right) file.
        new: PathBuf,
        #[command(flatten)]
        opts: DiffOpts,
    },
    /// Diff two in-memory strings.
    String {
        /// Old (left) content.
        old: String,
        /// New (right) content.
        new: String,
        /// Filename used for language detection (its extension drives the parser).
        #[arg(long, default_value = "snippet.txt")]
        filename: String,
        #[command(flatten)]
        opts: DiffOpts,
    },
    /// Diff changed files in a git repository (working tree, staged, or commit-to-commit).
    Git {
        /// Repository root.
        #[arg(default_value = ".")]
        repo: String,
        /// Restrict the review to a single file path (matches old or new filename).
        file: Option<String>,
        /// Old ref to compare from (default: HEAD, or HEAD~1 when --new is an explicit commit).
        #[arg(long)]
        old: Option<String>,
        /// New ref (default: the working tree). A commit ref does a commit-to-commit diff.
        #[arg(long, default_value = "")]
        new: String,
        /// Diff HEAD against the git index (staged files only).
        #[arg(long)]
        staged: bool,
        /// Diff against commits not yet pushed to the remote tracking branch.
        #[arg(long)]
        unpushed: bool,
        #[command(flatten)]
        opts: DiffOpts,
    },
    /// Inspect and manage the on-disk parse/diff cache.
    Cache {
        #[command(subcommand)]
        action: CacheAction,
    },
    /// Query local fuel/telemetry diagnostics (the DuckDB/SQLite analytics store).
    Diagnostics {
        #[command(subcommand)]
        action: DiagAction,
    },
    /// Perceptual diffs for non-text (image) assets.
    Assets {
        #[command(subcommand)]
        action: AssetAction,
    },
    /// Start the native live-server (keystroke diff/review over the JSON line protocol).
    /// Extra args are forwarded to the `intentumdiff-live-server` binary.
    LiveServer {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// Start the native LSP server for editor integration.
    /// Extra args are forwarded to the `intentumdiff-lsp-server` binary.
    LspServer {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// Parser/renderer plugins (list the bundled parsers; add/install/remove stay in the Python CLI).
    Plugins {
        #[command(subcommand)]
        action: Option<PluginAction>,
    },
    /// Pre-index a git repository to warm the symbol-index cache (faster cross-file detection).
    Index {
        /// Path to the git repository.
        #[arg(default_value = ".")]
        repo: String,
        /// Git ref to index.
        #[arg(long, default_value = "HEAD")]
        r#ref: String,
        /// Cache directory (the store lives at `<cache-path>/cache.db`).
        #[arg(long, default_value = ".intentumdiff-cache")]
        cache_path: PathBuf,
        /// Re-index even if a symbol index already exists for this commit.
        #[arg(long)]
        force: bool,
        /// Directory with the bundled parser `.wasm` + `parser_manifest.json`.
        #[arg(long)]
        wasm_dir: Option<PathBuf>,
    },
    /// Check a git diff against the repo's protected-config guardrail policy (intentumdiff.yaml).
    Guardrails {
        /// Repository path to check.
        #[arg(default_value = ".")]
        repo: String,
        /// Old ref to compare from.
        #[arg(long, default_value = "HEAD~1")]
        old: String,
        /// New ref to compare to.
        #[arg(long, default_value = "HEAD")]
        new: String,
        /// Exit with code 2 when any guardrail violation is found (for CI gating).
        #[arg(long)]
        strict: bool,
        #[command(flatten)]
        opts: DiffOpts,
    },
}

/// Shared perceptual-image-diff options (mirrors the Python CLI's asset args).
#[derive(clap::Args)]
struct AssetOpts {
    /// Directory for generated asset-diff artifacts.
    #[arg(long, default_value = ".intentumdiff/assets", global = true)]
    out: PathBuf,
    /// How to compare images with different dimensions.
    #[arg(long, default_value = "strict", value_parser = ["strict", "resize", "pad"])]
    dimension_policy: String,
    /// Per-pixel channel threshold before a pixel counts as changed.
    #[arg(long, default_value_t = 16)]
    pixel_threshold: u8,
    /// Minimum connected changed-pixel region (px) to keep.
    #[arg(long, default_value_t = 4)]
    region_min_area: usize,
    /// Whether alpha differences affect perceptual metrics.
    #[arg(long, default_value = "include", value_parser = ["include", "ignore"])]
    alpha_handling: String,
}

impl AssetOpts {
    fn options_json(&self) -> String {
        json!({
            "dimension_policy": self.dimension_policy,
            "pixel_threshold": self.pixel_threshold,
            "region_min_area": self.region_min_area,
            "alpha_handling": self.alpha_handling,
        })
        .to_string()
    }
}

#[derive(Subcommand)]
enum PluginAction {
    /// List the bundled parser plugins (from the manifest).
    List {
        /// Directory with `parser_manifest.json` (else the usual wasm-dir resolution).
        #[arg(long)]
        wasm_dir: Option<PathBuf>,
        /// Emit the manifest's parser map as JSON.
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
enum AssetAction {
    /// Compare two image files and generate perceptual artifacts.
    Diff {
        #[arg(long)]
        before: PathBuf,
        #[arg(long)]
        after: PathBuf,
        #[command(flatten)]
        opts: AssetOpts,
    },
    /// Discover changed image assets in a git range and diff each.
    Git {
        #[arg(long, default_value = ".")]
        repo: String,
        #[arg(long)]
        base: Option<String>,
        #[arg(long, default_value = "")]
        head: String,
        #[arg(long)]
        staged: bool,
        #[arg(long)]
        unpushed: bool,
        #[command(flatten)]
        opts: AssetOpts,
    },
}

/// The shared `--db FILE` analytics-store path (default `.intentumdiff/diagnostics.duckdb`, matching
/// the Python CLI). The store opens the provided DuckDB when available, else the SQLite fallback.
#[derive(clap::Args)]
struct DiagDb {
    #[arg(long, default_value = ".intentumdiff/diagnostics.duckdb", global = true)]
    db: PathBuf,
}

#[derive(Subcommand)]
enum DiagAction {
    /// Recent diagnostic runs + aggregate fuel by language (JSON).
    Summary {
        #[command(flatten)]
        db: DiagDb,
        #[arg(long, default_value_t = 10)]
        limit: i64,
    },
    /// Highest normalized parser-fuel hotspots (JSON).
    Hotspots {
        #[command(flatten)]
        db: DiagDb,
        #[arg(long, default_value_t = 20)]
        limit: i64,
    },
    /// Run a read-only SQL query against the diagnostics tables (JSON).
    Query {
        sql: String,
        #[command(flatten)]
        db: DiagDb,
    },
}

/// Shared options for the diff commands.
#[derive(clap::Args)]
struct DiffOpts {
    /// Directory holding the bundled parser `.wasm` + `parser_manifest.json`. Defaults to
    /// `$INTENTUMDIFF_WASM_DIR`, then a dir next to the binary, then the monorepo dev layout.
    #[arg(long)]
    wasm_dir: Option<PathBuf>,
    /// Emit the full SemanticDiff as JSON instead of a plain summary. (Rich output is parked —
    /// it lands later via the `gold` rich-in-Rust presentation layer.)
    #[arg(long)]
    json: bool,
}

/// The shared `--cache-path DIR` option (the store lives at `DIR/cache.db`, matching the
/// Python CLI's `Path(cache_path) / "cache.db"`).
#[derive(clap::Args)]
struct CacheDir {
    /// Directory holding `cache.db` (default: `.intentumdiff-cache`).
    #[arg(long, default_value = ".intentumdiff-cache", global = true)]
    cache_path: PathBuf,
}

#[derive(Subcommand)]
enum CacheAction {
    /// Show per-table entry counts, sizes, and hit/miss metrics.
    Stats {
        #[command(flatten)]
        dir: CacheDir,
    },
    /// Clear cached entries — all tables by default, or only the flagged ones.
    Clear {
        #[command(flatten)]
        dir: CacheDir,
        /// Clear the parse cache.
        #[arg(long)]
        parse: bool,
        /// Clear the diff cache.
        #[arg(long)]
        diff: bool,
        /// Clear the symbol-index cache.
        #[arg(long)]
        index: bool,
        /// Clear the hover-map cache.
        #[arg(long)]
        hover: bool,
    },
    /// List metadata rows for a cache table (JSON). TABLE: parse | diff | index | hover.
    List {
        table: String,
        #[command(flatten)]
        dir: CacheDir,
        /// Max rows to return.
        #[arg(long, default_value_t = 50)]
        limit: i64,
    },
    /// Export every entry (metadata + payload) for a table as a JSON array.
    Export {
        table: String,
        #[command(flatten)]
        dir: CacheDir,
    },
}

/// Map the short table aliases the CLI accepts to the store's physical table names
/// (mirrors the Python CLI's `_TABLE_ALIAS`).
fn resolve_table(alias: &str) -> Result<&'static str, String> {
    match alias {
        "parse" | "parse_cache" => Ok("parse_cache"),
        "diff" | "diff_cache" => Ok("diff_cache"),
        "index" | "symbol_index" | "symbol_index_cache" => Ok("symbol_index_cache"),
        "hover" | "hover_map" | "hover_map_cache" => Ok("hover_map_cache"),
        other => Err(format!(
            "unknown cache table {other:?} (expected: parse | diff | index | hover)"
        )),
    }
}

fn store_error_message(err: StoreError) -> String {
    match err {
        StoreError::Db(m) | StoreError::Value(m) => m,
    }
}

/// Open the SQLite cache at `<cache_path>/cache.db`, or `None` when the file is absent
/// (a read command on a fresh checkout should say so, not create an empty DB).
fn open_existing(cache_path: &Path) -> Result<Option<SqliteStore>, String> {
    let db = cache_path.join("cache.db");
    if !db.exists() {
        return Ok(None);
    }
    SqliteStore::open(db.to_string_lossy().as_ref(), 30, 500)
        .map(Some)
        .map_err(store_error_message)
}

fn run_cache(action: CacheAction) -> Result<(), String> {
    match action {
        CacheAction::Stats { dir } => {
            let Some(store) = open_existing(&dir.cache_path)? else {
                println!("No cache found at {}", dir.cache_path.join("cache.db").display());
                return Ok(());
            };
            let stats: serde_json::Value =
                serde_json::from_str(&store.stats().map_err(store_error_message)?)
                    .map_err(|e| e.to_string())?;
            let metrics: serde_json::Value =
                serde_json::from_str(&store.metrics().map_err(store_error_message)?)
                    .map_err(|e| e.to_string())?;
            let _ = store.close();
            println!(
                "{:<20} {:>8} {:>12} {:>8} {:>8} {:>9}",
                "table", "entries", "size_bytes", "hits", "misses", "hit_rate"
            );
            for table in ["parse_cache", "diff_cache", "symbol_index_cache", "hover_map_cache"] {
                let s = &stats[table];
                let m = &metrics[table];
                println!(
                    "{:<20} {:>8} {:>12} {:>8} {:>8} {:>8.1}%",
                    table,
                    s["count"].as_i64().unwrap_or(0),
                    s["size_bytes"].as_i64().unwrap_or(0),
                    m["hits"].as_i64().unwrap_or(0),
                    m["misses"].as_i64().unwrap_or(0),
                    m["hit_rate_pct"].as_f64().unwrap_or(0.0),
                );
            }
        }
        CacheAction::Clear { dir, parse, diff, index, hover } => {
            let Some(store) = open_existing(&dir.cache_path)? else {
                println!("No cache found at {}", dir.cache_path.join("cache.db").display());
                return Ok(());
            };
            // No table flags = clear everything (matches the Python CLI default).
            let all = !(parse || diff || index || hover);
            store
                .clear(parse || all, diff || all, index || all, hover || all)
                .map_err(store_error_message)?;
            let _ = store.close();
            println!("Cleared cache at {}", dir.cache_path.join("cache.db").display());
        }
        CacheAction::List { table, dir, limit } => {
            let table = resolve_table(&table)?;
            let Some(store) = open_existing(&dir.cache_path)? else {
                println!("[]");
                return Ok(());
            };
            let json = store
                .list_entries(table, None, None, None, None, None, limit, false)
                .map_err(store_error_message)?;
            let _ = store.close();
            println!("{json}");
        }
        CacheAction::Export { table, dir } => {
            let table = resolve_table(&table)?;
            let Some(store) = open_existing(&dir.cache_path)? else {
                println!("[]");
                return Ok(());
            };
            let json = store.export_entries(table).map_err(store_error_message)?;
            let _ = store.close();
            println!("{json}");
        }
    }
    Ok(())
}

/// Resolve the bundled-parser directory: `--wasm-dir` > `$INTENTUMDIFF_WASM_DIR` > a `wasm/` dir next
/// to the binary (the shipped shape) > the monorepo dev layout (`src/intentumdiff/wasm`, found by
/// walking the exe's ancestors). Every candidate is verified by `parser_manifest.json` — mirrors
/// the native live-server's resolver so both binaries find parsers identically.
fn resolve_wasm_dir(explicit: Option<&Path>) -> String {
    let has_manifest = |dir: &Path| dir.join("parser_manifest.json").exists();
    if let Some(dir) = explicit {
        return dir.to_string_lossy().into_owned();
    }
    if let Ok(dir) = std::env::var("INTENTUMDIFF_WASM_DIR") {
        if !dir.trim().is_empty() {
            return dir;
        }
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(exe_dir) = exe.parent() {
            let shipped = exe_dir.join("wasm");
            if has_manifest(&shipped) {
                return shipped.to_string_lossy().into_owned();
            }
            for ancestor in exe_dir.ancestors() {
                let dev = ancestor.join("src").join("intentumdiff").join("wasm");
                if has_manifest(&dev) {
                    return dev.to_string_lossy().into_owned();
                }
            }
        }
    }
    String::new()
}

/// Diff two in-memory contents via the native all-language engine (`live_diff_contents`, which
/// resolves the parser from *filename*'s extension against the bundled manifest).
fn run_diff(filename: &str, old: &str, new: &str, opts: &DiffOpts) -> Result<(), String> {
    let wasm_dir = resolve_wasm_dir(opts.wasm_dir.as_deref());
    let raw = intentumdiff_rust_core::live_server::live_diff_contents_impl(
        ".", filename, old, new, "{}", &wasm_dir,
    )?;
    let result: serde_json::Value = serde_json::from_str(&raw).map_err(|e| e.to_string())?;

    // The native path returns `{diff: <SemanticDiff>}` or `{fallback: <reason>}` (no Python
    // fallback exists here, so a fallback marker means the engine could not serve this input).
    if let Some(reason) = result.get("fallback").and_then(|v| v.as_str()) {
        return Err(format!(
            "the native engine did not produce a diff for {filename:?}: {reason}"
        ));
    }
    let diff = result.get("diff").unwrap_or(&result);

    if opts.json {
        println!("{}", serde_json::to_string_pretty(diff).map_err(|e| e.to_string())?);
        return Ok(());
    }

    // Parked-rich plain summary: change count + one line per change. The full rich presentation
    // lands later via `gold`.
    let changes = diff.get("changes").and_then(|v| v.as_array());
    let engine = diff
        .get("metadata")
        .and_then(|m| m.get("rust_core"))
        .and_then(|r| r.get("engine"))
        .and_then(|v| v.as_str())
        .unwrap_or("native");
    match changes {
        Some(changes) => {
            println!("{} change(s) [engine: {engine}]", changes.len());
            for change in changes {
                let change_type = change.get("change_type").and_then(|v| v.as_str()).unwrap_or("?");
                // `description` is the human "what" ("Update integer('1') -> integer('2')"); fall
                // back to a node label when a change carries no description.
                let label = change
                    .get("description")
                    .or_else(|| change.get("new_label"))
                    .or_else(|| change.get("label"))
                    .and_then(|v| v.as_str())
                    .or_else(|| {
                        change
                            .get("new_node")
                            .or_else(|| change.get("old_node"))
                            .and_then(|n| n.get("label"))
                            .and_then(|v| v.as_str())
                    })
                    .unwrap_or("");
                println!("  {change_type:<14} {label}");
            }
        }
        None => println!("0 change(s) [engine: {engine}]"),
    }
    Ok(())
}

/// Review the changed files in a git repo via the native engine (`live_handle_review`, which
/// resolves changed sources + parsers and diffs each file). `new_ref` sentinels: `""` = working
/// tree, `:staged` = index, `:unpushed` = commits not yet pushed; anything else = commit-to-commit.
#[allow(clippy::too_many_arguments)]
fn run_git(
    repo: &str,
    file: Option<&str>,
    old: Option<&str>,
    new: &str,
    staged: bool,
    unpushed: bool,
    opts: &DiffOpts,
) -> Result<(), String> {
    let new_ref: &str = if staged {
        ":staged"
    } else if unpushed {
        ":unpushed"
    } else {
        new
    };
    // Default old ref: HEAD when new is the working tree / a scope sentinel, HEAD~1 for an
    // explicit new commit (so `git --new <sha>` means "the commit before it → it").
    let old_ref: String = old.map(str::to_owned).unwrap_or_else(|| {
        if new_ref.is_empty() || new_ref.starts_with(':') {
            "HEAD".to_owned()
        } else {
            "HEAD~1".to_owned()
        }
    });
    let wasm_dir = resolve_wasm_dir(opts.wasm_dir.as_deref());
    let raw = intentumdiff_rust_core::live_server::live_handle_review_impl(
        repo, &old_ref, new_ref, "{}", &wasm_dir,
    )?;
    let result: serde_json::Value = serde_json::from_str(&raw).map_err(|e| e.to_string())?;

    if let Some(reason) = result.get("fallback").and_then(|v| v.as_str()) {
        return Err(format!("the native engine did not produce a review: {reason}"));
    }
    let commit_diff = result.get("commit_diff").unwrap_or(&result);

    if opts.json {
        println!("{}", serde_json::to_string_pretty(commit_diff).map_err(|e| e.to_string())?);
        return Ok(());
    }

    // Parked-rich plain summary: per-file change counts + cross-file / guardrail totals.
    let empty = Vec::new();
    let file_diffs = commit_diff.get("file_diffs").and_then(|v| v.as_array()).unwrap_or(&empty);
    let filename_of = |d: &serde_json::Value| -> String {
        d.get("new_filename")
            .or_else(|| d.get("old_filename"))
            .and_then(|v| v.as_str())
            .unwrap_or("<unknown>")
            .to_owned()
    };
    let mut total = 0usize;
    let mut shown = 0usize;
    for d in file_diffs {
        let name = filename_of(d);
        if let Some(f) = file {
            if name != f && d.get("old_filename").and_then(|v| v.as_str()) != Some(f) {
                continue;
            }
        }
        let n = d.get("changes").and_then(|v| v.as_array()).map(|a| a.len()).unwrap_or(0);
        total += n;
        shown += 1;
        println!("{name}: {n} change(s)");
    }
    let cross = commit_diff.get("cross_file_changes").and_then(|v| v.as_array()).map(|a| a.len()).unwrap_or(0);
    let guardrails = commit_diff.get("guardrail_violations").and_then(|v| v.as_array()).map(|a| a.len()).unwrap_or(0);
    println!(
        "{shown} file(s), {total} change(s){}{}",
        if cross > 0 { format!(", {cross} cross-file change(s)") } else { String::new() },
        if guardrails > 0 { format!(", {guardrails} guardrail violation(s)") } else { String::new() },
    );
    Ok(())
}

/// Open the analytics store at *db*, or `None` when the file is absent (a read command should say
/// so rather than create an empty DB).
fn open_analytics(db: &Path) -> Result<Option<AnalyticsStore>, String> {
    if !db.exists() {
        return Ok(None);
    }
    AnalyticsStore::open(db.to_string_lossy().as_ref())
        .map(Some)
        .map_err(store_error_message)
}

fn run_diagnostics(action: DiagAction) -> Result<(), String> {
    let missing = |db: &Path| println!("No diagnostics database at {}", db.display());
    match action {
        DiagAction::Summary { db, limit } => {
            let Some(store) = open_analytics(&db.db)? else {
                missing(&db.db);
                return Ok(());
            };
            let runs = store.recent_diagnostic_runs(limit).map_err(store_error_message)?;
            let langs = store.fuel_by_language(limit).map_err(store_error_message)?;
            let _ = store.close();
            println!("{{\"recent_runs\":{runs},\"fuel_by_language\":{langs}}}");
        }
        DiagAction::Hotspots { db, limit } => {
            let Some(store) = open_analytics(&db.db)? else {
                missing(&db.db);
                return Ok(());
            };
            let hotspots = store.top_fuel_hotspots(limit).map_err(store_error_message)?;
            let _ = store.close();
            println!("{hotspots}");
        }
        DiagAction::Query { sql, db } => {
            let Some(store) = open_analytics(&db.db)? else {
                missing(&db.db);
                return Ok(());
            };
            let rows = store.query_readonly(&sql).map_err(store_error_message)?;
            let _ = store.close();
            println!("{rows}");
        }
    }
    Ok(())
}

/// Call a C-ABI dispatch handler in-process and return the `result` value, surfacing the
/// `{ok:false, error}` envelope as an `Err`. Used for engine ops without a public `*_impl`.
fn dispatch_result(name: &str, args: &[serde_json::Value]) -> Result<serde_json::Value, String> {
    let envelope: serde_json::Value =
        serde_json::from_str(&intentumdiff_rust_core::c_abi::dispatch(name, args))
            .map_err(|e| e.to_string())?;
    if envelope.get("ok").and_then(|v| v.as_bool()) != Some(true) {
        return Err(envelope
            .get("error")
            .and_then(|v| v.as_str())
            .unwrap_or("engine call failed")
            .to_owned());
    }
    Ok(envelope.get("result").cloned().unwrap_or(serde_json::Value::Null))
}

fn run_assets(action: AssetAction) -> Result<(), String> {
    let result = match action {
        AssetAction::Diff { before, after, opts } => dispatch_result(
            "diff_asset_image",
            &[
                json!(before.to_string_lossy()),
                json!(after.to_string_lossy()),
                json!(opts.out.to_string_lossy()),
                json!(opts.options_json()),
            ],
        )?,
        AssetAction::Git { repo, base, head, staged, unpushed, opts } => {
            let head_ref: String = if staged {
                ":staged".into()
            } else if unpushed {
                ":unpushed".into()
            } else {
                head
            };
            let base_ref = base.unwrap_or_else(|| "HEAD".into());
            dispatch_result(
                "diff_git_assets",
                &[
                    json!(repo),
                    json!(base_ref),
                    json!(head_ref),
                    json!(opts.out.to_string_lossy()),
                    json!(opts.options_json()),
                ],
            )?
        }
    };
    // Asset diffs are structured artifacts (metrics + generated file paths) — emit the JSON.
    println!("{}", serde_json::to_string_pretty(&result).map_err(|e| e.to_string())?);
    Ok(())
}

/// Check a git diff against the repo's guardrail policy. The native review loads the
/// `intentumdiff.yaml` policy itself (walking from the repo root) and attaches violations; this
/// surfaces them and, under `--strict`, exits 2 for CI gating.
fn run_guardrails(repo: &str, old: &str, new: &str, strict: bool, opts: &DiffOpts) -> Result<(), String> {
    let wasm_dir = resolve_wasm_dir(opts.wasm_dir.as_deref());
    let raw =
        intentumdiff_rust_core::live_server::live_handle_review_impl(repo, old, new, "{}", &wasm_dir)?;
    let result: serde_json::Value = serde_json::from_str(&raw).map_err(|e| e.to_string())?;
    if let Some(reason) = result.get("fallback").and_then(|v| v.as_str()) {
        return Err(format!("guardrail policy could not be evaluated natively: {reason}"));
    }
    let commit_diff = result.get("commit_diff").unwrap_or(&result);
    let empty = Vec::new();
    let violations = commit_diff
        .get("guardrail_violations")
        .and_then(|v| v.as_array())
        .unwrap_or(&empty);

    if opts.json {
        println!("{}", serde_json::to_string_pretty(violations).map_err(|e| e.to_string())?);
    } else if violations.is_empty() {
        println!("No guardrail violations.");
    } else {
        println!("{} guardrail violation(s):", violations.len());
        for v in violations {
            let severity = v.get("severity").and_then(|s| s.as_str()).unwrap_or("");
            let rule = v.get("rule_id").or_else(|| v.get("rule")).and_then(|s| s.as_str()).unwrap_or("");
            let msg = v
                .get("message")
                .or_else(|| v.get("description"))
                .and_then(|s| s.as_str())
                .unwrap_or("");
            println!("  [{severity}] {rule}  {msg}");
        }
    }

    if strict && !violations.is_empty() {
        // CI-gating exit code, matching the Python CLI's --strict.
        std::process::exit(2);
    }
    Ok(())
}

/// List the bundled parser plugins from `parser_manifest.json`. (Third-party plugin management —
/// add/install/remove — is pip-ecosystem I/O and stays in the Python CLI → intentumdiff-python.)
fn run_plugins(action: Option<PluginAction>) -> Result<(), String> {
    let (wasm_dir_opt, as_json) = match action {
        Some(PluginAction::List { wasm_dir, json }) => (wasm_dir, json),
        None => (None, false),
    };
    let wasm_dir = resolve_wasm_dir(wasm_dir_opt.as_deref());
    let manifest_path = Path::new(&wasm_dir).join("parser_manifest.json");
    let text = std::fs::read_to_string(&manifest_path)
        .map_err(|e| format!("read {}: {e}", manifest_path.display()))?;
    let manifest: serde_json::Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
    let parsers = manifest
        .get("parsers")
        .and_then(|v| v.as_object())
        .ok_or("manifest has no `parsers` map")?;

    if as_json {
        println!("{}", serde_json::to_string_pretty(&manifest["parsers"]).map_err(|e| e.to_string())?);
        return Ok(());
    }
    println!("{} bundled parser plugin(s):", parsers.len());
    let mut langs: Vec<&String> = parsers.keys().collect();
    langs.sort();
    for lang in langs {
        let entry = &parsers[lang];
        let exts = entry
            .get("extensions")
            .and_then(|v| v.as_array())
            .map(|a| a.iter().filter_map(|e| e.as_str()).collect::<Vec<_>>().join(" "))
            .unwrap_or_default();
        println!("  {lang:<16} {exts}");
    }
    Ok(())
}

/// Locate a sibling native binary: `$env_var` override > next to this exe (the shipped shape at
/// the split) > the monorepo dev layout (`<repo>/crates/<dev_crate>/target/<profile>/<bin>`) > PATH.
fn resolve_sibling_bin(bin: &str, env_var: &str, dev_crate: &str) -> String {
    if let Ok(p) = std::env::var(env_var) {
        if !p.trim().is_empty() {
            return p;
        }
    }
    let exe_name = if cfg!(windows) { format!("{bin}.exe") } else { bin.to_owned() };
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let sibling = dir.join(&exe_name);
            if sibling.exists() {
                return sibling.to_string_lossy().into_owned();
            }
            let profile = dir
                .file_name()
                .map(|f| f.to_string_lossy().into_owned())
                .unwrap_or_else(|| "debug".to_owned());
            for ancestor in dir.ancestors() {
                let candidate = ancestor
                    .join("crates")
                    .join(dev_crate)
                    .join("target")
                    .join(&profile)
                    .join(&exe_name);
                if candidate.exists() {
                    return candidate.to_string_lossy().into_owned();
                }
            }
        }
    }
    bin.to_owned() // last resort: rely on PATH
}

/// Launch a sibling server binary, forwarding *args*, and propagate its exit code. The CLI stays
/// attached for the server's lifetime (a thin launcher — the split ships the binaries side by side).
fn run_server(bin_path: &str, args: &[String]) -> Result<(), String> {
    let status = std::process::Command::new(bin_path)
        .args(args)
        .status()
        .map_err(|e| format!("failed to launch {bin_path}: {e}"))?;
    std::process::exit(status.code().unwrap_or(1));
}

/// Run `git -C <repo> <args>` and return trimmed stdout, or an `Err` with stderr.
fn git_out(repo: &str, args: &[&str]) -> Result<String, String> {
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .output()
        .map_err(|e| format!("git: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "git {}: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Pre-index a git repo: parse every resolvable file at *ref* to a tree, build the symbol +
/// reference tables, and store them under `make_index_key(repo_root, commit_sha)` — the exact key
/// the differ's cross-file path looks up, so subsequent diffs of that commit skip the rebuild.
fn run_index(
    repo: &str,
    git_ref: &str,
    cache_path: &Path,
    force: bool,
    wasm_dir_opt: Option<&Path>,
) -> Result<(), String> {
    let repo_root = git_out(repo, &["rev-parse", "--show-toplevel"])?.trim().to_owned();
    let commit_sha = git_out(repo, &["rev-parse", git_ref])?.trim().to_owned();
    let short = &commit_sha[..commit_sha.len().min(8)];
    let index_key = intentumdiff_rust_core::make_index_key(&repo_root, &commit_sha);

    let db = cache_path.join("cache.db");
    let store = SqliteStore::open(db.to_string_lossy().as_ref(), 30, 500).map_err(store_error_message)?;

    if !force && store.get_symbol_index(&index_key).map_err(store_error_message)?.is_some() {
        let _ = store.close();
        println!("Already indexed {repo_root} @ {short} (use --force to rebuild).");
        return Ok(());
    }

    let wasm_dir = resolve_wasm_dir(wasm_dir_opt);
    let files = git_out(repo, &["ls-tree", "-r", "--name-only", git_ref])?;
    let mut entries: Vec<serde_json::Value> = Vec::new();
    let mut skipped = 0usize;
    for file in files.lines().filter(|l| !l.is_empty()) {
        let spec = format!("{git_ref}:{file}");
        let content = match git_out(repo, &["show", spec.as_str()]) {
            Ok(c) => c,
            Err(_) => {
                skipped += 1;
                continue;
            }
        };
        // Unresolvable extension or a parse failure -> skip the file (not the whole index).
        match intentumdiff_rust_core::parse_to_tree(file, &content, "{}", &wasm_dir) {
            Ok(parsed) => {
                let v: serde_json::Value = serde_json::from_str(&parsed).map_err(|e| e.to_string())?;
                entries.push(json!({
                    "filename": file,
                    "language": v.get("language").cloned().unwrap_or_default(),
                    "tree": v.get("tree").cloned().unwrap_or_default(),
                }));
            }
            Err(_) => skipped += 1,
        }
    }
    let indexed = entries.len();
    let files_json = serde_json::Value::Array(entries).to_string();

    // build_symbol_table / build_reference_table return JSON strings; the store persists those.
    let symbols = dispatch_result("build_symbol_table", &[json!(files_json.clone())])?;
    let refs = dispatch_result("build_reference_table", &[json!(files_json)])?;
    let symbols_str = serde_json::to_string(&symbols).map_err(|e| e.to_string())?;
    let refs_str = serde_json::to_string(&refs).map_err(|e| e.to_string())?;
    store
        .put_symbol_index(&index_key, &symbols_str, &refs_str, indexed as i64)
        .map_err(store_error_message)?;
    let _ = store.close();

    println!("Indexed {indexed} file(s) ({skipped} skipped) at {repo_root} @ {short} -> {}", db.display());
    Ok(())
}

fn run(cli: Cli) -> Result<(), String> {
    match cli.command {
        Command::Engine => {
            println!("intentumdiff {ENGINE_BANNER}");
            Ok(())
        }
        Command::File { old, new, opts } => {
            let old_content = std::fs::read_to_string(&old)
                .map_err(|e| format!("read {}: {e}", old.display()))?;
            let new_content = std::fs::read_to_string(&new)
                .map_err(|e| format!("read {}: {e}", new.display()))?;
            // Language is detected from the new file's name (its extension).
            let filename = new.file_name().map(|f| f.to_string_lossy().into_owned())
                .unwrap_or_else(|| "file".to_string());
            run_diff(&filename, &old_content, &new_content, &opts)
        }
        Command::String { old, new, filename, opts } => run_diff(&filename, &old, &new, &opts),
        Command::Git { repo, file, old, new, staged, unpushed, opts } => {
            run_git(&repo, file.as_deref(), old.as_deref(), &new, staged, unpushed, &opts)
        }
        Command::Cache { action } => run_cache(action),
        Command::Diagnostics { action } => run_diagnostics(action),
        Command::Assets { action } => run_assets(action),
        Command::Guardrails { repo, old, new, strict, opts } => {
            run_guardrails(&repo, &old, &new, strict, &opts)
        }
        Command::Plugins { action } => run_plugins(action),
        Command::Index { repo, r#ref, cache_path, force, wasm_dir } => {
            run_index(&repo, &r#ref, &cache_path, force, wasm_dir.as_deref())
        }
        Command::LiveServer { args } => {
            let bin = resolve_sibling_bin(
                "intentumdiff-live-server",
                "INTENTUMDIFF_LIVE_SERVER_BIN",
                "live-server",
            );
            run_server(&bin, &args)
        }
        Command::LspServer { args } => {
            let bin = resolve_sibling_bin(
                "intentumdiff-lsp-server",
                "INTENTUMDIFF_LSP_SERVER_BIN",
                "lsp-server",
            );
            run_server(&bin, &args)
        }
    }
}

fn main() -> ExitCode {
    match run(Cli::parse()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("error: {message}");
            ExitCode::FAILURE
        }
    }
}
