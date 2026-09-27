use intentumdiff_rust_core::cache_store::SqliteStore;
use serde_json::Value;

#[test]
fn source_expected_cache_query_corpus() {
    let fixture: Value = serde_json::from_str(include_str!("fixtures/cache_filtering.json")).unwrap();
    let dir = std::env::temp_dir().join(format!("idf_cache_corpus_{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("cache.db");
    {
        let store = SqliteStore::open(path.to_str().unwrap(), 30, 500).unwrap();
        for row in fixture["rows"].as_array().unwrap() {
            store.put_diff(row["key"].as_str().unwrap(), "{}", row["language"].as_str().unwrap(), row["old_filename"].as_str().unwrap(), row["new_filename"].as_str().unwrap()).unwrap();
        }
        {
            let conn = rusqlite::Connection::open(&path).unwrap();
            for row in fixture["rows"].as_array().unwrap() {
                conn.execute("UPDATE diff_cache SET created_at=?1, size_bytes=?2 WHERE key=?3", rusqlite::params![row["created_at"].as_i64(), row["size_bytes"].as_i64(), row["key"].as_str()]).unwrap();
            }
        }
        for case in fixture["cases"].as_array().unwrap() {
            let q = &case["query"];
            let rows: Value = serde_json::from_str(&store.list_entries_filtered("diff_cache", q["language"].as_str(), q["since"].as_i64(), q["before"].as_i64(), q["min_size"].as_i64(), q["max_size"].as_i64(), q["limit"].as_i64().unwrap_or(50), q["file_glob"].as_str()).unwrap()).unwrap();
            let keys: Vec<_> = rows.as_array().unwrap().iter().map(|r|r["key"].clone()).collect();
            assert_eq!(serde_json::json!(keys), case["expected_keys"], "{}", case["name"]);
        }
    }
    std::fs::remove_dir_all(dir).unwrap();
}
