//! Index-engine Wasm plugin.
//!
//! Implements the ``index-engine`` WIT world. All CST/symbol processing is
//! delegated to the shared `index-engine-lib` crate so the exact same logic
//! runs both here (as a Wasm plugin) and natively in `rust-core-host`'s
//! certified commit path — no behavioural fork. Mirrors the
//! `sql-parser` → `sql-parser-lib` split.
//!
//! Exposes three functions:
//!
//! * ``build_symbol_table`` — flat qualified-name → [SymbolDefinition] table.
//! * ``diff_symbol_tables`` — MOVE_TO_MODULE / SPLIT_MODULE / CROSS_FILE_RENAME.
//! * ``build_reference_table`` — label → [ReferenceUsage] table.

wit_bindgen::generate!({
    path: "wit/plugin.wit",
    world: "index-engine-plugin",
});

use crate::exports::intentumdiff::plugin::index_engine::Guest;

struct IndexEngine;

impl Guest for IndexEngine {
    fn build_symbol_table(files_json: String) -> String {
        index_engine_lib::build_symbol_table_impl(&files_json)
    }
    fn diff_symbol_tables(old_json: String, new_json: String) -> String {
        index_engine_lib::diff_symbol_tables_impl(&old_json, &new_json)
    }
    fn build_reference_table(files_json: String) -> String {
        index_engine_lib::build_reference_table_impl(&files_json)
    }
}

export!(IndexEngine);
