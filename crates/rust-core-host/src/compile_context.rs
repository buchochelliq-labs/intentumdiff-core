//! Pure compile-database interpretation. Hosts supply bytes and path context.
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

fn windows(path: &str) -> bool {
    path.as_bytes().get(1) == Some(&b':') || (path.starts_with("\\\\") || path.starts_with("//"))
}
fn absolute(path: &str) -> bool {
    path.starts_with('/') || windows(path)
}
fn normalize(path: &str) -> String {
    let value = if windows(path) {
        path.replace('\\', "/")
    } else {
        path.to_owned()
    };
    let mut parts = Vec::new();
    for part in value.split('/') {
        match part {
            "" | "." => (),
            ".." => {
                if parts.last().is_some_and(|p| *p != "..") {
                    parts.pop();
                } else if !absolute(&value) {
                    parts.push(part);
                }
            }
            _ => parts.push(part),
        }
    }
    format!(
        "{}{}",
        if value.starts_with("//") {
            "//"
        } else if value.starts_with('/') {
            "/"
        } else {
            ""
        },
        parts.join("/")
    )
}
fn resolve(base: &str, path: &str) -> String {
    normalize(&if absolute(path) {
        path.to_owned()
    } else {
        format!("{base}/{path}")
    })
}
fn key(path: &str) -> String {
    let normalized = normalize(path);
    if windows(path) {
        normalized.to_lowercase()
    } else {
        normalized
    }
}
fn relative(path: &str, cwd: &str) -> String {
    let path = normalize(path);
    let base = normalize(cwd);
    if key(&path) == key(&base) {
        return ".".into();
    }
    let prefix = format!("{}/", base.trim_end_matches('/'));
    let comparison_prefix = format!("{}/", key(&base).trim_end_matches('/'));
    if key(&path).starts_with(&comparison_prefix) && path.len() >= prefix.len() {
        path[prefix.len()..].to_owned()
    } else {
        path
    }
}

/// Tokenize command text without executing it. Quote/escape semantics are selected by target path.
pub fn command_arguments(command: &str, is_windows: bool) -> Result<Vec<String>, String> {
    let mut args = Vec::new();
    let mut arg = String::new();
    let mut quote = None;
    let mut active = false;
    let mut chars = command.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' && quote != Some('\'') {
            if is_windows {
                let mut count = 1;
                while chars.peek() == Some(&'\\') {
                    chars.next();
                    count += 1;
                }
                if chars.peek() == Some(&'"') {
                    arg.extend(std::iter::repeat_n('\\', count / 2));
                    chars.next();
                    if count % 2 == 1 {
                        arg.push('"');
                    } else {
                        quote = if quote == Some('"') { None } else { Some('"') };
                    }
                } else {
                    arg.extend(std::iter::repeat_n('\\', count));
                }
            } else if quote == Some('"') {
                match chars.peek() {
                    Some('"' | '\\' | '$' | '`' | '\n') => {
                        let next = chars.next().unwrap();
                        if next != '\n' {
                            arg.push(next);
                        }
                    }
                    _ => arg.push('\\'),
                }
            } else {
                let next = chars.next().ok_or("unterminated command escape")?;
                if next != '\n' {
                    arg.push(next);
                }
            }
            active = true;
            continue;
        }
        if c == '"' || (c == '\'' && !is_windows) {
            if is_windows && c == '"' && quote == Some(c) && chars.peek() == Some(&'"') {
                chars.next();
                arg.push(c);
            } else if quote == Some(c) {
                quote = None;
            } else if quote.is_none() {
                quote = Some(c);
            } else {
                arg.push(c);
            }
            active = true;
            continue;
        }
        if c.is_whitespace() && quote.is_none() {
            if active {
                args.push(std::mem::take(&mut arg));
                active = false;
            }
        } else {
            arg.push(c);
            active = true;
        }
    }
    if quote.is_some() {
        return Err("unterminated command quote".into());
    }
    if active {
        args.push(arg);
    }
    Ok(args)
}
fn flags(args: &[String], prefix: &str) -> Vec<String> {
    let mut result = Vec::new();
    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];
        if arg == prefix && i + 1 < args.len() {
            i += 1;
            result.push(args[i].clone());
        } else if let Some(v) = arg.strip_prefix(prefix).filter(|v| !v.is_empty()) {
            result.push(v.to_owned());
        }
        i += 1;
    }
    result
}

/// Resolve only an exact source path. Missing or ambiguous matches have no context.
pub fn metadata(
    database: &Value,
    database_path: &str,
    filename: &str,
    cwd: &str,
) -> Result<Option<Value>, String> {
    let entries = database
        .as_array()
        .ok_or("compile database must be an array")?;
    let db = resolve(cwd, database_path);
    let parent = db.rsplit_once('/').map(|(p, _)| p).unwrap_or(cwd);
    let target = resolve(cwd, filename);
    let mut selected = None;
    for entry in entries {
        let Some(file) = entry
            .get("file")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
        else {
            continue;
        };
        let directory = resolve(
            parent,
            entry
                .get("directory")
                .and_then(Value::as_str)
                .unwrap_or("."),
        );
        let path = resolve(&directory, file);
        if key(&path) != key(&target) {
            continue;
        }
        if selected.is_some() {
            return Ok(None);
        }
        selected = Some((entry, directory, path));
    }
    let Some((entry, directory, path)) = selected else {
        return Ok(None);
    };
    let args = if let Some(raw) = entry.get("arguments") {
        raw.as_array()
            .ok_or("compile arguments must be an array")?
            .iter()
            .map(|v| {
                v.as_str()
                    .map(str::to_owned)
                    .ok_or("compile arguments must be strings")
            })
            .collect::<Result<Vec<_>, _>>()?
    } else if let Some(command) = entry.get("command").and_then(Value::as_str) {
        command_arguments(command, windows(&target))?
    } else {
        return Ok(None);
    };
    if args.is_empty() {
        return Ok(None);
    }
    let mut defines = flags(&args, "-D");
    defines.extend(flags(&args, "/D"));
    let mut includes = flags(&args, "-I");
    includes.extend(flags(&args, "/I"));
    includes.extend(flags(&args, "-isystem"));
    let standard = args
        .iter()
        .find_map(|s| s.strip_prefix("-std=").or_else(|| s.strip_prefix("/std:")));
    let mut result = json!({"database":relative(&db,cwd),"directory":relative(&directory,cwd),"file":relative(&path,cwd),
        "arguments":args,"defines":defines,"include_dirs":includes,"standard":standard});
    let payload = json!({"directory":directory,"file":result["file"],"arguments":result["arguments"],"defines":result["defines"],"include_dirs":result["include_dirs"],"standard":result["standard"]});
    result["fingerprint"] =
        json!(&hex::encode(Sha256::digest(payload.to_string().as_bytes()))[..16]);
    Ok(Some(result))
}

pub(crate) fn request(request: &Value) -> Result<Value, String> {
    let string = |name| {
        request
            .get(name)
            .and_then(Value::as_str)
            .ok_or_else(|| format!("missing {name}"))
    };
    let language = string("language")?.to_lowercase();
    if !["c", "cpp", "c++", "cxx"].contains(&language.as_str()) {
        return Ok(Value::Null);
    }
    metadata(
        request.get("database").ok_or("missing database")?,
        string("database_path")?,
        string("filename")?,
        string("cwd")?,
    )
    .map(|v| v.unwrap_or(Value::Null))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_unrelated_basename_and_preserves_case() {
        let db = json!([{"directory":"/repo","file":"a/main.cpp","arguments":["c++","-DA"]}]);
        assert!(
            metadata(&db, "/repo/compile_commands.json", "b/main.cpp", "/repo")
                .unwrap()
                .is_none()
        );
        assert!(
            metadata(&db, "/repo/compile_commands.json", "a/Main.cpp", "/repo")
                .unwrap()
                .is_none()
        );
    }
    #[test]
    fn quotes_and_windows_paths() {
        assert_eq!(
            command_arguments("cc -I\"include spaces\" -DNAME='hello world'", false).unwrap(),
            vec!["cc", "-Iinclude spaces", "-DNAME=hello world"]
        );
        let db = json!([{"directory":"C:\\Repo","file":"src\\a.cpp","command":"cl /I\"C:\\include dir\" /DDEBUG /std:c++20 src\\a.cpp"}]);
        let v = metadata(
            &db,
            "C:\\Repo\\compile_commands.json",
            "SRC\\A.cpp",
            "C:\\Repo",
        )
        .unwrap()
        .unwrap();
        assert_eq!(v["include_dirs"], json!(["C:\\include dir"]));
        assert_eq!(v["defines"], json!(["DEBUG"]));
        assert!(command_arguments("cc 'unfinished", false).is_err());
    }
}

#[cfg(test)]
mod regression_tests {
    use super::*;
    #[test]
    fn outside_root_is_not_relative_and_directory_affects_fingerprint() {
        let db = json!([{"directory":"/repo2","file":"x.cpp","arguments":["cc","-Iinclude"]}]);
        let v = metadata(&db, "/repo/compile_commands.json", "/repo2/x.cpp", "/repo")
            .unwrap()
            .unwrap();
        assert_eq!(v["file"], "/repo2/x.cpp");
        assert_eq!(v["directory"], "/repo2");
        let db2 =
            json!([{"directory":"/other","file":"/repo2/x.cpp","arguments":["cc","-Iinclude"]}]);
        let w = metadata(&db2, "/repo/compile_commands.json", "/repo2/x.cpp", "/repo")
            .unwrap()
            .unwrap();
        assert_ne!(v["fingerprint"], w["fingerprint"]);
    }
    #[test]
    fn platform_paths_preserve_exactness() {
        let db = json!([{"directory":"/repo","file":"dir\\main.cpp","arguments":["cc"]}]);
        assert!(
            metadata(&db, "/repo/compile_commands.json", "dir/main.cpp", "/repo")
                .unwrap()
                .is_none()
        );
        let unc =
            json!([{"directory":"\\\\server\\share\\repo","file":"SRC\\A.cpp","arguments":["cl"]}]);
        assert!(metadata(
            &unc,
            "\\\\SERVER\\Share\\Repo\\compile_commands.json",
            "src\\a.cpp",
            "\\\\SERVER\\Share\\Repo"
        )
        .unwrap()
        .is_some());
        assert_eq!(
            command_arguments("cl /DNAME=\"a\"\"b\" a.cpp", true).unwrap()[1],
            "/DNAME=a\"b"
        );
    }
}

/// Native-host filesystem adapter. Interpretation remains in `metadata`.
/// Compile context is metadata only; this does not invoke a compiler or alter parsing.
pub fn discover(
    filename: &str,
    language: &str,
    cwd: &std::path::Path,
) -> Result<Option<Value>, String> {
    if !["c", "cpp", "c++", "cxx"].contains(&language.to_lowercase().as_str())
        || filename.starts_with('<')
    {
        return Ok(None);
    }
    let target = cwd.join(filename);
    let Some(parent) = target.parent() else {
        return Ok(None);
    };
    for directory in parent.ancestors() {
        let database = directory.join("compile_commands.json");
        if !database.is_file() {
            continue;
        }
        let Ok(raw) = std::fs::read_to_string(&database) else {
            return Ok(None);
        };
        let Ok(entries) = serde_json::from_str(&raw) else {
            return Ok(None);
        };
        return metadata(
            &entries,
            &database.to_string_lossy(),
            filename,
            &cwd.to_string_lossy(),
        );
    }
    Ok(None)
}
