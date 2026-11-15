use std::collections::{BTreeMap, HashSet};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use base64::Engine;
use serde::{Deserialize, Serialize};

use crate::agent_resume::AgentSessionRefKind;
use crate::persist::{PaneAgentSessionSnapshot, WorkspaceSnapshot};

pub const HERDR_LINK_DIR: &str = "herdrLink";
pub const HERDR_LINK_MANIFEST: &str = "manifest.json";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HerdrLinkManifest {
    pub version: u32,
    pub source_workspace_cwd: String,
    pub source_workspace_uri: String,
    pub workspace_snapshot: WorkspaceSnapshot,
    #[serde(default)]
    pub conversation_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HerdrLinkSummary {
    pub link_dir: PathBuf,
    pub workspace_cwd: PathBuf,
    pub conversation_count: usize,
    pub compressed_db_count: usize,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct SummariesBundle {
    #[serde(default)]
    columns: Vec<String>,
    #[serde(default)]
    rows: Vec<BTreeMap<String, serde_json::Value>>,
    #[serde(default)]
    parents: Vec<(String, String)>,
}

pub fn has_herdr_link(workspace_cwd: &Path) -> bool {
    workspace_cwd
        .join(HERDR_LINK_DIR)
        .join(HERDR_LINK_MANIFEST)
        .is_file()
}

pub fn default_agy_data_dir() -> Option<PathBuf> {
    let home = std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)?;
    Some(home.join(".gemini").join("antigravity-cli"))
}

pub fn clean_path_string(path: &Path) -> String {
    let s = path.to_string_lossy();
    let stripped = if let Some(rest) = s.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{}", rest)
    } else if let Some(rest) = s.strip_prefix(r"\\?\") {
        rest.to_string()
    } else {
        s.into_owned()
    };
    let trimmed = stripped.trim_end_matches(['/', '\\']);
    if trimmed.is_empty() {
        stripped
    } else {
        trimmed.to_string()
    }
}

pub fn path_to_file_uri(path: &Path) -> String {
    let normalized = clean_path_string(path).replace('\\', "/");
    let encoded = percent_encode_uri_path(&normalized);
    if encoded.starts_with('/') {
        format!("file://{encoded}")
    } else {
        format!("file:///{encoded}")
    }
}

fn percent_encode_uri_path(path: &str) -> String {
    let mut out = String::with_capacity(path.len());
    for ch in path.chars() {
        match ch {
            '%' => out.push_str("%25"),
            ' ' => out.push_str("%20"),
            '#' => out.push_str("%23"),
            '?' => out.push_str("%3F"),
            _ => out.push(ch),
        }
    }
    out
}

fn normalize_path_key(path: &str) -> String {
    let cleaned = clean_path_string(Path::new(path)).replace('\\', "/");
    if cfg!(windows) {
        cleaned.to_lowercase()
    } else {
        cleaned
    }
}

fn remap_path_with_prefix(old_path: &Path, old_root: &Path, new_root: &Path) -> PathBuf {
    let old_p = clean_path_string(old_path).replace('\\', "/");
    let old_r = clean_path_string(old_root).replace('\\', "/");
    let old_p_key = normalize_path_key(&old_p);
    let old_r_key = normalize_path_key(&old_r);

    if old_p_key == old_r_key {
        return new_root.to_path_buf();
    }
    let prefix = format!("{old_r_key}/");
    if old_p_key.starts_with(&prefix) && old_p.len() > prefix.len() {
        let rel = &old_p[prefix.len()..];
        let mut out = new_root.to_path_buf();
        for comp in rel.split('/').filter(|c| !c.is_empty()) {
            out.push(comp);
        }
        return out;
    }
    new_root.to_path_buf()
}

fn read_varint(data: &[u8], mut pos: usize) -> Option<(u64, usize)> {
    let mut val: u64 = 0;
    let mut shift = 0u32;
    while pos < data.len() {
        let byte = data[pos];
        pos += 1;
        val |= ((byte & 0x7f) as u64) << shift;
        if (byte & 0x80) == 0 {
            return Some((val, pos));
        }
        shift += 7;
        if shift >= 64 {
            return None;
        }
    }
    None
}

fn write_varint(mut val: u64, out: &mut Vec<u8>) {
    loop {
        let mut byte = (val & 0x7f) as u8;
        val >>= 7;
        if val != 0 {
            byte |= 0x80;
        }
        out.push(byte);
        if val == 0 {
            break;
        }
    }
}

fn contains_subslice(haystack: &[u8], needle: &[u8]) -> bool {
    !needle.is_empty() && haystack.windows(needle.len()).any(|w| w == needle)
}

fn replace_all_bytes(data: &[u8], replacements: &[(&[u8], &[u8])]) -> Vec<u8> {
    let mut buf = data.to_vec();
    for &(old, new) in replacements {
        if old.is_empty() || old == new {
            continue;
        }
        let mut out = Vec::with_capacity(buf.len());
        let mut i = 0usize;
        while i < buf.len() {
            if i + old.len() <= buf.len() && &buf[i..i + old.len()] == old {
                out.extend_from_slice(new);
                i += old.len();
            } else {
                out.push(buf[i]);
                i += 1;
            }
        }
        buf = out;
    }
    buf
}

fn is_nested_proto_with_pattern(data: &[u8], replacements: &[(&[u8], &[u8])]) -> bool {
    if data.is_empty() {
        return false;
    }
    let mut pos = 0usize;
    let mut saw_wire2_with_pattern = false;
    while pos < data.len() {
        let Some((tag, next_pos)) = read_varint(data, pos) else {
            return false;
        };
        pos = next_pos;
        let field_num = tag >> 3;
        let wire_type = (tag & 0x07) as u8;
        if field_num == 0 {
            return false;
        }
        match wire_type {
            0 => {
                let Some((_, np)) = read_varint(data, pos) else {
                    return false;
                };
                pos = np;
            }
            1 => {
                if pos + 8 > data.len() {
                    return false;
                }
                pos += 8;
            }
            5 => {
                if pos + 4 > data.len() {
                    return false;
                }
                pos += 4;
            }
            2 => {
                let Some((len, np)) = read_varint(data, pos) else {
                    return false;
                };
                pos = np;
                let len = len as usize;
                if pos.checked_add(len).is_none_or(|end| end > data.len()) {
                    return false;
                }
                let payload = &data[pos..pos + len];
                if replacements
                    .iter()
                    .any(|(old, _)| contains_subslice(payload, old))
                {
                    saw_wire2_with_pattern = true;
                }
                pos += len;
            }
            _ => return false,
        }
    }
    saw_wire2_with_pattern
}

/// Rewrites Protobuf wire bytes in place while automatically updating length-delimited
/// varint headers at every nesting level.
pub fn rewrite_protobuf_bytes(data: &[u8], replacements: &[(&[u8], &[u8])]) -> Vec<u8> {
    if !replacements
        .iter()
        .any(|(old, _)| contains_subslice(data, old))
    {
        return data.to_vec();
    }

    let mut out = Vec::with_capacity(data.len() + 32);
    let mut pos = 0usize;
    while pos < data.len() {
        let tag_start = pos;
        let Some((tag, after_tag)) = read_varint(data, pos) else {
            return replace_all_bytes(data, replacements);
        };
        let field_num = tag >> 3;
        let wire_type = (tag & 0x07) as u8;
        if field_num == 0 {
            return replace_all_bytes(data, replacements);
        }
        match wire_type {
            0 => {
                let Some((_, np)) = read_varint(data, after_tag) else {
                    return replace_all_bytes(data, replacements);
                };
                out.extend_from_slice(&data[tag_start..np]);
                pos = np;
            }
            1 => {
                if after_tag + 8 > data.len() {
                    return replace_all_bytes(data, replacements);
                }
                out.extend_from_slice(&data[tag_start..after_tag + 8]);
                pos = after_tag + 8;
            }
            5 => {
                if after_tag + 4 > data.len() {
                    return replace_all_bytes(data, replacements);
                }
                out.extend_from_slice(&data[tag_start..after_tag + 4]);
                pos = after_tag + 4;
            }
            2 => {
                let Some((len, payload_start)) = read_varint(data, after_tag) else {
                    return replace_all_bytes(data, replacements);
                };
                let len = len as usize;
                let Some(payload_end) = payload_start.checked_add(len) else {
                    return replace_all_bytes(data, replacements);
                };
                if payload_end > data.len() {
                    return replace_all_bytes(data, replacements);
                }
                let payload = &data[payload_start..payload_end];
                out.extend_from_slice(&data[tag_start..after_tag]);
                if replacements
                    .iter()
                    .any(|(old, _)| contains_subslice(payload, old))
                {
                    let rewritten = if is_nested_proto_with_pattern(payload, replacements) {
                        rewrite_protobuf_bytes(payload, replacements)
                    } else {
                        replace_all_bytes(payload, replacements)
                    };
                    write_varint(rewritten.len() as u64, &mut out);
                    out.extend_from_slice(&rewritten);
                } else {
                    out.extend_from_slice(&data[after_tag..payload_end]);
                }
                pos = payload_end;
            }
            _ => return replace_all_bytes(data, replacements),
        }
    }
    out
}

fn extract_proto_field1_string(entry: &[u8]) -> Option<&str> {
    let mut pos = 0usize;
    while pos < entry.len() {
        let (tag, after_tag) = read_varint(entry, pos)?;
        let field_num = tag >> 3;
        let wire_type = (tag & 0x07) as u8;
        match wire_type {
            0 => pos = read_varint(entry, after_tag)?.1,
            1 => pos = after_tag + 8,
            5 => pos = after_tag + 4,
            2 => {
                let (len, payload_start) = read_varint(entry, after_tag)?;
                let end = payload_start + len as usize;
                if end > entry.len() {
                    return None;
                }
                if field_num == 1 {
                    return std::str::from_utf8(&entry[payload_start..end]).ok();
                }
                pos = end;
            }
            _ => return None,
        }
    }
    None
}

pub fn update_jetbox_summaries_pb(
    pb_path: &Path,
    updates: &[(String, Vec<u8>)],
) -> std::io::Result<()> {
    if updates.is_empty() {
        return Ok(());
    }
    let updated_ids: HashSet<&str> = updates.iter().map(|(id, _)| id.as_str()).collect();
    let mut out = Vec::new();

    if let Ok(existing) = std::fs::read(pb_path) {
        let mut pos = 0usize;
        let mut parsed_ok = true;
        while pos < existing.len() {
            let entry_start = pos;
            let Some((tag, after_tag)) = read_varint(&existing, pos) else {
                parsed_ok = false;
                break;
            };
            if (tag & 0x07) != 2 {
                parsed_ok = false;
                break;
            }
            let Some((len, payload_start)) = read_varint(&existing, after_tag) else {
                parsed_ok = false;
                break;
            };
            let end = payload_start + len as usize;
            if end > existing.len() {
                parsed_ok = false;
                break;
            }
            if (tag >> 3) == 1 {
                let payload = &existing[payload_start..end];
                let should_skip = extract_proto_field1_string(payload)
                    .is_some_and(|id| updated_ids.contains(id));
                if !should_skip {
                    out.extend_from_slice(&existing[entry_start..end]);
                }
            } else {
                out.extend_from_slice(&existing[entry_start..end]);
            }
            pos = end;
        }
        if !parsed_ok {
            out.clear();
        }
    }

    for (conv_id, raw_summary) in updates {
        let mut inner = Vec::with_capacity(conv_id.len() + raw_summary.len() + 8);
        inner.push(0x0a);
        write_varint(conv_id.len() as u64, &mut inner);
        inner.extend_from_slice(conv_id.as_bytes());
        inner.push(0x12);
        write_varint(raw_summary.len() as u64, &mut inner);
        inner.extend_from_slice(raw_summary);

        out.push(0x0a);
        write_varint(inner.len() as u64, &mut out);
        out.extend_from_slice(&inner);
    }

    if let Some(parent) = pb_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = pb_path.with_extension("pb.tmp");
    std::fs::write(&tmp, &out)?;
    std::fs::rename(&tmp, pb_path)?;
    Ok(())
}

fn find_python_executable() -> Option<&'static str> {
    for candidate in ["python", "python3", "py"] {
        if let Ok(output) = Command::new(candidate)
            .arg("-c")
            .arg("import sqlite3, gzip, json")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .output()
        {
            if output.status.success() {
                return Some(candidate);
            }
        }
    }
    None
}

fn run_python_script(script: &str, stdin_json: &str) -> Result<String, String> {
    let py = find_python_executable()
        .ok_or_else(|| "python/python3 with sqlite3 is required for herdrLink".to_string())?;
    let mut cmd = Command::new(py);
    if py == "py" {
        cmd.arg("-3");
    }
    let mut child = cmd
        .arg("-c")
        .arg(script)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("failed to launch {py}: {e}"))?;

    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(stdin_json.as_bytes())
            .map_err(|e| format!("failed writing stdin to {py}: {e}"))?;
    }
    let output = child
        .wait_with_output()
        .map_err(|e| format!("failed waiting on {py}: {e}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("herdrLink python helper failed: {stderr}"));
    }
    String::from_utf8(output.stdout).map_err(|e| format!("invalid utf8 from python helper: {e}"))
}

const EXPORT_AGY_PYTHON: &str = r#"
import base64, gzip, json, os, pathlib, shutil, sqlite3, sys, tempfile

req = json.loads(sys.stdin.read())
agy_dir = pathlib.Path(req["agy_dir"])
out_conv_dir = pathlib.Path(req["out_conv_dir"])
out_conv_dir.mkdir(parents=True, exist_ok=True)
workspace_uri = req["workspace_uri"]
workspace_cwd = req["workspace_cwd"]
seed_ids = set(req.get("seed_ids") or [])

def norm(s):
    return s.replace("\\", "/").rstrip("/").lower()

target_uri_l = norm(workspace_uri)
target_cwd_l = norm(workspace_cwd)

summaries_db = agy_dir / "conversation_summaries.db"
columns = []
rows_by_id = {}
parents = []

if summaries_db.is_file():
    conn = sqlite3.connect(f"file:{summaries_db}?mode=ro", uri=True)
    cur = conn.cursor()
    all_parents = []
    if cur.execute("SELECT 1 FROM sqlite_master WHERE type='table' AND name='conversation_parents'").fetchone():
        all_parents = cur.execute("SELECT conversation_id, parent_id FROM conversation_parents").fetchall()
    cur.execute("SELECT * FROM conversation_summaries ORDER BY last_modified_time DESC")
    columns = [d[0] for d in cur.description]
    all_rows = cur.fetchall()

    matched_ids = set(seed_ids)
    row_dicts = {}
    for raw_row in all_rows:
        d = {}
        for col, val in zip(columns, raw_row):
            if isinstance(val, bytes):
                d[col] = {"__b64__": base64.b64encode(val).decode("ascii")}
            else:
                d[col] = val
        cid = d.get("conversation_id", "")
        row_dicts[cid] = d
        uris = str(d.get("workspace_uris", "")).replace("\\", "/").lower()
        if (target_uri_l and target_uri_l in uris) or (target_cwd_l and target_cwd_l in uris):
            matched_ids.add(cid)

    changed = True
    while changed:
        changed = False
        for cid, pid in all_parents:
            if (cid in matched_ids and pid not in matched_ids) or (pid in matched_ids and cid not in matched_ids):
                matched_ids.add(cid)
                matched_ids.add(pid)
                changed = True
        for cid, d in row_dicts.items():
            pid = d.get("parent_conversation_id") or ""
            if pid and pid in matched_ids and cid not in matched_ids:
                matched_ids.add(cid)
                changed = True

    for cid, d in row_dicts.items():
        if cid in matched_ids:
            rows_by_id[cid] = d
    parents = [(cid, pid) for cid, pid in all_parents if cid in matched_ids or pid in matched_ids]
    conn.close()

conv_ids = list(rows_by_id.keys())
for sid in sorted(seed_ids):
    if sid not in rows_by_id:
        conv_ids.append(sid)

compressed_count = 0
for cid in conv_ids:
    src_db = agy_dir / "conversations" / f"{cid}.db"
    if not src_db.is_file():
        continue
    fd, tmp_path = tempfile.mkstemp(suffix=".db")
    os.close(fd)
    try:
        src = sqlite3.connect(f"file:{src_db}?mode=ro", uri=True)
        dst = sqlite3.connect(tmp_path)
        src.backup(dst)
        dst.close()
        src.close()
        gz_path = out_conv_dir / f"{cid}.db.gz"
        with open(tmp_path, "rb") as f_in, gzip.open(gz_path, "wb", compresslevel=6) as f_out:
            shutil.copyfileobj(f_in, f_out)
        compressed_count += 1
    finally:
        if os.path.exists(tmp_path):
            os.remove(tmp_path)

bundle = {
    "columns": columns,
    "rows": [rows_by_id[cid] for cid in conv_ids if cid in rows_by_id],
    "parents": parents,
    "conversation_ids": conv_ids,
    "compressed_db_count": compressed_count,
}
print(json.dumps(bundle))
"#;

const DECOMPRESS_CONVS_PYTHON: &str = r#"
import base64, gzip, json, pathlib, shutil, sqlite3, sys

req = json.loads(sys.stdin.read())
gz_dir = pathlib.Path(req["gz_dir"])
dst_conv_dir = pathlib.Path(req["dst_conv_dir"])
dst_conv_dir.mkdir(parents=True, exist_ok=True)

blobs = {}
for gz_path in sorted(gz_dir.glob("*.db.gz")):
    cid = gz_path.name[:-6]
    dst_db = dst_conv_dir / f"{cid}.db"
    with gzip.open(gz_path, "rb") as f_in, open(dst_db, "wb") as f_out:
        shutil.copyfileobj(f_in, f_out)
    conn = sqlite3.connect(str(dst_db))
    cur = conn.cursor()
    if cur.execute("SELECT 1 FROM sqlite_master WHERE type='table' AND name='trajectory_metadata_blob'").fetchone():
        per_conv = {}
        for row_id, data in cur.execute("SELECT id, data FROM trajectory_metadata_blob").fetchall():
            if data:
                per_conv[row_id] = base64.b64encode(data).decode("ascii")
        if per_conv:
            blobs[cid] = per_conv
    conn.close()

print(json.dumps({"blobs": blobs}))
"#;

const APPLY_LINK_DB_PYTHON: &str = r#"
import base64, json, pathlib, sqlite3, sys

req = json.loads(sys.stdin.read())
agy_dir = pathlib.Path(req["agy_dir"])
dst_conv_dir = agy_dir / "conversations"
dst_conv_dir.mkdir(parents=True, exist_ok=True)

for cid, blob_map in (req.get("rewritten_blobs") or {}).items():
    db_path = dst_conv_dir / f"{cid}.db"
    if not db_path.is_file():
        continue
    conn = sqlite3.connect(str(db_path))
    for row_id, b64_val in blob_map.items():
        conn.execute("UPDATE trajectory_metadata_blob SET data = ? WHERE id = ?", (base64.b64decode(b64_val), row_id))
    conn.commit()
    conn.close()

summaries_db = agy_dir / "conversation_summaries.db"
conn = sqlite3.connect(str(summaries_db))
conn.execute("""
CREATE TABLE IF NOT EXISTS `conversation_summaries` (
    `conversation_id` text,
    `title` text NOT NULL DEFAULT "",
    `preview` text NOT NULL DEFAULT "",
    `step_count` integer NOT NULL DEFAULT 0,
    `last_modified_time` datetime NOT NULL,
    `workspace_uris` text NOT NULL,
    `status` text NOT NULL DEFAULT "",
    `source` text NOT NULL DEFAULT "",
    `project_id` text NOT NULL DEFAULT "",
    `agent_name` text NOT NULL DEFAULT "",
    `parent_conversation_id` text NOT NULL DEFAULT "",
    `nesting_depth` integer NOT NULL DEFAULT 0,
    `battle_id` text NOT NULL DEFAULT "",
    `winning_conversation_id` text NOT NULL DEFAULT "",
    `not_fully_idle` numeric NOT NULL DEFAULT false,
    `killed` numeric NOT NULL DEFAULT false,
    `last_user_input_time` datetime NOT NULL,
    `last_user_input_step_index` integer NOT NULL DEFAULT -1,
    `app_data_dir` text NOT NULL DEFAULT "",
    raw_summary BLOB,
    group_id TEXT NOT NULL DEFAULT '',
    PRIMARY KEY (`conversation_id`)
)
""")
conn.execute("""
CREATE TABLE IF NOT EXISTS conversation_parents (
    conversation_id TEXT PRIMARY KEY,
    parent_id TEXT NOT NULL
) WITHOUT ROWID
""")

existing_cols = {r[1] for r in conn.execute("PRAGMA table_info(conversation_summaries)").fetchall()}
for row in (req.get("rows") or []):
    cols = [c for c in row.keys() if c in existing_cols]
    if not cols:
        continue
    vals = []
    for c in cols:
        v = row[c]
        if isinstance(v, dict) and "__b64__" in v:
            vals.append(base64.b64decode(v["__b64__"]))
        else:
            vals.append(v)
    placeholders = ",".join(["?"] * len(cols))
    col_sql = ",".join(f"`{c}`" for c in cols)
    conn.execute(f"INSERT OR REPLACE INTO conversation_summaries ({col_sql}) VALUES ({placeholders})", vals)

for cid, pid in (req.get("parents") or []):
    conn.execute("INSERT OR REPLACE INTO conversation_parents (conversation_id, parent_id) VALUES (?, ?)", (cid, pid))

conn.commit()
conn.close()
print(json.dumps({"ok": True}))
"#;

fn collect_snapshot_agy_ids(snapshot: &WorkspaceSnapshot) -> Vec<String> {
    let mut ids = Vec::new();
    let mut seen = HashSet::new();
    for tab in &snapshot.tabs {
        for pane in tab.panes.values() {
            if let Some(session) = &pane.agent_session {
                if session.agent == "agy"
                    && !session.value.is_empty()
                    && session.value != "continue"
                    && seen.insert(session.value.clone())
                {
                    ids.push(session.value.clone());
                }
            }
        }
    }
    ids
}

fn copy_dir_filtered(
    src: &Path,
    dst: &Path,
    text_replacements: Option<&[(String, String)]>,
) -> Result<(), String> {
    if !src.exists() {
        return Ok(());
    }
    std::fs::create_dir_all(dst)
        .map_err(|e| format!("failed creating {}: {e}", dst.display()))?;
    let entries =
        std::fs::read_dir(src).map_err(|e| format!("failed reading {}: {e}", src.display()))?;
    for entry in entries.flatten() {
        let name = entry.file_name();
        let src_path = entry.path();
        let dst_path = dst.join(&name);
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_dir() {
            copy_dir_filtered(&src_path, &dst_path, text_replacements)?;
        } else if file_type.is_file() {
            let is_text = src_path
                .extension()
                .and_then(|e| e.to_str())
                .is_some_and(|ext| matches!(ext, "md" | "jsonl" | "json" | "txt"));
            if is_text {
                if let Some(replacements) = text_replacements {
                    if let Ok(mut content) = std::fs::read_to_string(&src_path) {
                        for (old, new) in replacements {
                            if !old.is_empty() && old != new {
                                content = content.replace(old.as_str(), new.as_str());
                            }
                        }
                        std::fs::write(&dst_path, content)
                            .map_err(|e| format!("failed writing {}: {e}", dst_path.display()))?;
                        continue;
                    }
                }
            }
            std::fs::copy(&src_path, &dst_path)
                .map_err(|e| format!("failed copying {}: {e}", dst_path.display()))?;
        }
    }
    Ok(())
}

fn export_history_jsonl(
    src_history: &Path,
    dst_history: &Path,
    conv_ids: &HashSet<String>,
    workspace_cwd: &str,
) -> Result<(), String> {
    if !src_history.is_file() {
        return Ok(());
    }
    let content = std::fs::read_to_string(src_history)
        .map_err(|e| format!("failed reading {}: {e}", src_history.display()))?;
    let target_key = normalize_path_key(workspace_cwd);
    let mut kept = String::new();
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let Ok(val) = serde_json::from_str::<serde_json::Value>(trimmed) else {
            continue;
        };
        let cid_match = val
            .get("conversationId")
            .and_then(|v| v.as_str())
            .is_some_and(|cid| conv_ids.contains(cid));
        let ws_match = val
            .get("workspace")
            .and_then(|v| v.as_str())
            .is_some_and(|ws| normalize_path_key(ws) == target_key);
        if cid_match || ws_match {
            kept.push_str(trimmed);
            kept.push('\n');
        }
    }
    if !kept.is_empty() {
        std::fs::write(dst_history, kept)
            .map_err(|e| format!("failed writing {}: {e}", dst_history.display()))?;
    }
    Ok(())
}

fn merge_history_jsonl(
    src_history: &Path,
    dst_history: &Path,
    new_workspace_cwd: &str,
) -> Result<(), String> {
    if !src_history.is_file() {
        return Ok(());
    }
    let src_content = std::fs::read_to_string(src_history)
        .map_err(|e| format!("failed reading {}: {e}", src_history.display()))?;
    let existing = std::fs::read_to_string(dst_history).unwrap_or_default();

    let mut seen_keys: HashSet<(String, u64, String)> = HashSet::new();
    let mut out_lines: Vec<String> = Vec::new();
    for line in existing.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Ok(val) = serde_json::from_str::<serde_json::Value>(trimmed) {
            let cid = val
                .get("conversationId")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string();
            let ts = val.get("timestamp").and_then(|v| v.as_u64()).unwrap_or(0);
            let display = val
                .get("display")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string();
            seen_keys.insert((cid, ts, display));
        }
        out_lines.push(trimmed.to_string());
    }

    for line in src_content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let Ok(mut val) = serde_json::from_str::<serde_json::Value>(trimmed) else {
            continue;
        };
        if let Some(obj) = val.as_object_mut() {
            obj.insert(
                "workspace".to_string(),
                serde_json::Value::String(new_workspace_cwd.to_string()),
            );
        }
        let cid = val
            .get("conversationId")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();
        let ts = val.get("timestamp").and_then(|v| v.as_u64()).unwrap_or(0);
        let display = val
            .get("display")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();
        if seen_keys.insert((cid, ts, display)) {
            if let Ok(encoded) = serde_json::to_string(&val) {
                out_lines.push(encoded);
            }
        }
    }

    let mut joined = out_lines.join("\n");
    if !joined.is_empty() {
        joined.push('\n');
    }
    if let Some(parent) = dst_history.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("failed creating {}: {e}", parent.display()))?;
    }
    std::fs::write(dst_history, joined)
        .map_err(|e| format!("failed writing {}: {e}", dst_history.display()))
}

fn build_text_replacements(
    old_cwd: &str,
    new_cwd: &str,
    old_uri: &str,
    new_uri: &str,
) -> Vec<(String, String)> {
    let mut pairs = Vec::new();
    let old_slash = old_cwd.replace('\\', "/");
    let new_slash = new_cwd.replace('\\', "/");
    let old_backslash = old_cwd.replace('/', "\\");
    let new_backslash = new_cwd.replace('/', "\\");
    if !old_uri.is_empty() && old_uri != new_uri {
        pairs.push((old_uri.to_string(), new_uri.to_string()));
    }
    if !old_backslash.is_empty() && old_backslash != new_backslash {
        pairs.push((old_backslash, new_backslash));
    }
    if !old_slash.is_empty() && old_slash != new_slash {
        pairs.push((old_slash, new_slash));
    }
    pairs
}

pub fn save_herdr_link(
    workspace_cwd: &Path,
    snapshot: WorkspaceSnapshot,
    agy_data_dir: Option<&Path>,
) -> Result<HerdrLinkSummary, String> {
    let link_dir = workspace_cwd.join(HERDR_LINK_DIR);
    let agy_out = link_dir.join("agy");
    let conv_out = agy_out.join("conversations");
    let brain_out = agy_out.join("brain");
    let ann_out = agy_out.join("annotations");
    std::fs::create_dir_all(&conv_out)
        .map_err(|e| format!("failed creating {}: {e}", conv_out.display()))?;

    let source_cwd = clean_path_string(workspace_cwd);
    let source_uri = path_to_file_uri(workspace_cwd);
    let seed_ids = collect_snapshot_agy_ids(&snapshot);

    let resolved_agy = agy_data_dir.map(PathBuf::from).or_else(default_agy_data_dir);
    let mut conv_ids: Vec<String> = seed_ids.clone();
    let mut compressed_db_count = 0usize;

    if let Some(agy_dir) = resolved_agy.filter(|d| d.exists()) {
        let req = serde_json::json!({
            "agy_dir": clean_path_string(&agy_dir),
            "out_conv_dir": clean_path_string(&conv_out),
            "workspace_uri": source_uri,
            "workspace_cwd": source_cwd,
            "seed_ids": seed_ids,
        });
        let raw_json = run_python_script(EXPORT_AGY_PYTHON, &req.to_string())?;
        let parsed: serde_json::Value = serde_json::from_str(&raw_json)
            .map_err(|e| format!("failed parsing export output: {e}"))?;

        if let Some(ids) = parsed.get("conversation_ids").and_then(|v| v.as_array()) {
            conv_ids = ids
                .iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect();
        }
        compressed_db_count = parsed
            .get("compressed_db_count")
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as usize;

        let bundle = SummariesBundle {
            columns: serde_json::from_value(
                parsed
                    .get("columns")
                    .cloned()
                    .unwrap_or_else(|| serde_json::Value::Array(Vec::new())),
            )
            .unwrap_or_default(),
            rows: serde_json::from_value(
                parsed
                    .get("rows")
                    .cloned()
                    .unwrap_or_else(|| serde_json::Value::Array(Vec::new())),
            )
            .unwrap_or_default(),
            parents: serde_json::from_value(
                parsed
                    .get("parents")
                    .cloned()
                    .unwrap_or_else(|| serde_json::Value::Array(Vec::new())),
            )
            .unwrap_or_default(),
        };

        let summaries_json = serde_json::to_string_pretty(&bundle)
            .map_err(|e| format!("failed serializing summaries.json: {e}"))?;
        std::fs::write(agy_out.join("summaries.json"), summaries_json)
            .map_err(|e| format!("failed writing summaries.json: {e}"))?;

        let id_set: HashSet<String> = conv_ids.iter().cloned().collect();
        for cid in &conv_ids {
            let src_brain = agy_dir.join("brain").join(cid);
            if src_brain.is_dir() {
                copy_dir_filtered(&src_brain, &brain_out.join(cid), None)?;
            }
            let src_ann = agy_dir.join("annotations").join(format!("{cid}.pbtxt"));
            if src_ann.is_file() {
                std::fs::create_dir_all(&ann_out)
                    .map_err(|e| format!("failed creating {}: {e}", ann_out.display()))?;
                std::fs::copy(&src_ann, ann_out.join(format!("{cid}.pbtxt")))
                    .map_err(|e| format!("failed copying annotation {cid}: {e}"))?;
            }
        }
        export_history_jsonl(
            &agy_dir.join("history.jsonl"),
            &agy_out.join("history.jsonl"),
            &id_set,
            &source_cwd,
        )?;
    }

    let manifest = HerdrLinkManifest {
        version: 1,
        source_workspace_cwd: source_cwd,
        source_workspace_uri: source_uri,
        workspace_snapshot: snapshot,
        conversation_ids: conv_ids.clone(),
    };
    let manifest_json = serde_json::to_string_pretty(&manifest)
        .map_err(|e| format!("failed serializing manifest.json: {e}"))?;
    std::fs::write(link_dir.join(HERDR_LINK_MANIFEST), manifest_json)
        .map_err(|e| format!("failed writing manifest.json: {e}"))?;

    Ok(HerdrLinkSummary {
        link_dir,
        workspace_cwd: workspace_cwd.to_path_buf(),
        conversation_count: conv_ids.len(),
        compressed_db_count,
    })
}

pub fn load_and_link_herdr_link(
    workspace_cwd: &Path,
    agy_data_dir: Option<&Path>,
) -> Result<(WorkspaceSnapshot, HerdrLinkSummary), String> {
    let link_dir = workspace_cwd.join(HERDR_LINK_DIR);
    let manifest_path = link_dir.join(HERDR_LINK_MANIFEST);
    let manifest_str = std::fs::read_to_string(&manifest_path)
        .map_err(|e| format!("failed reading {}: {e}", manifest_path.display()))?;
    let manifest: HerdrLinkManifest = serde_json::from_str(&manifest_str)
        .map_err(|e| format!("failed parsing {}: {e}", manifest_path.display()))?;

    let new_cwd_str = clean_path_string(workspace_cwd);
    let new_uri = path_to_file_uri(workspace_cwd);
    let old_cwd_path = Path::new(&manifest.source_workspace_cwd);
    let text_pairs = build_text_replacements(
        &manifest.source_workspace_cwd,
        &new_cwd_str,
        &manifest.source_workspace_uri,
        &new_uri,
    );
    let byte_pairs_owned: Vec<(Vec<u8>, Vec<u8>)> = text_pairs
        .iter()
        .map(|(a, b)| (a.as_bytes().to_vec(), b.as_bytes().to_vec()))
        .collect();
    let byte_pairs: Vec<(&[u8], &[u8])> = byte_pairs_owned
        .iter()
        .map(|(a, b)| (a.as_slice(), b.as_slice()))
        .collect();

    let agy_in = link_dir.join("agy");
    let conv_gz_dir = agy_in.join("conversations");
    let resolved_agy = agy_data_dir.map(PathBuf::from).or_else(default_agy_data_dir);
    let mut restored_conv_count = manifest.conversation_ids.len();
    let mut db_count = 0usize;

    if let Some(agy_dir) = resolved_agy {
        std::fs::create_dir_all(&agy_dir)
            .map_err(|e| format!("failed creating {}: {e}", agy_dir.display()))?;

        let mut rewritten_blobs: BTreeMap<String, BTreeMap<String, String>> = BTreeMap::new();
        if conv_gz_dir.is_dir() {
            let dec_req = serde_json::json!({
                "gz_dir": clean_path_string(&conv_gz_dir),
                "dst_conv_dir": clean_path_string(&agy_dir.join("conversations")),
            });
            let dec_out = run_python_script(DECOMPRESS_CONVS_PYTHON, &dec_req.to_string())?;
            let dec_val: serde_json::Value = serde_json::from_str(&dec_out)
                .map_err(|e| format!("failed parsing decompress output: {e}"))?;
            if let Some(blobs_obj) = dec_val.get("blobs").and_then(|v| v.as_object()) {
                for (cid, inner_val) in blobs_obj {
                    db_count += 1;
                    let Some(inner_obj) = inner_val.as_object() else {
                        continue;
                    };
                    let mut per_conv = BTreeMap::new();
                    for (row_id, b64_val) in inner_obj {
                        let Some(b64_str) = b64_val.as_str() else {
                            continue;
                        };
                        let Ok(raw_bytes) =
                            base64::engine::general_purpose::STANDARD.decode(b64_str)
                        else {
                            continue;
                        };
                        let updated = rewrite_protobuf_bytes(&raw_bytes, &byte_pairs);
                        per_conv.insert(
                            row_id.clone(),
                            base64::engine::general_purpose::STANDARD.encode(updated),
                        );
                    }
                    if !per_conv.is_empty() {
                        rewritten_blobs.insert(cid.clone(), per_conv);
                    }
                }
            }
        }

        let summaries_path = agy_in.join("summaries.json");
        let mut bundle: SummariesBundle = if summaries_path.is_file() {
            let s = std::fs::read_to_string(&summaries_path)
                .map_err(|e| format!("failed reading summaries.json: {e}"))?;
            serde_json::from_str(&s)
                .map_err(|e| format!("failed parsing summaries.json: {e}"))?
        } else {
            SummariesBundle::default()
        };

        let mut pb_updates: Vec<(String, Vec<u8>)> = Vec::new();
        let new_uris_json = serde_json::to_string(&vec![&new_uri]).unwrap_or_default();

        for row in &mut bundle.rows {
            row.insert(
                "workspace_uris".to_string(),
                serde_json::Value::String(new_uris_json.clone()),
            );
            let cid = row
                .get("conversation_id")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string();
            if let Some(raw_val) = row.get_mut("raw_summary") {
                if let Some(b64_str) = raw_val
                    .as_object()
                    .and_then(|o| o.get("__b64__"))
                    .and_then(|v| v.as_str())
                {
                    if let Ok(raw_bytes) =
                        base64::engine::general_purpose::STANDARD.decode(b64_str)
                    {
                        let rewritten = rewrite_protobuf_bytes(&raw_bytes, &byte_pairs);
                        let new_b64 = base64::engine::general_purpose::STANDARD.encode(&rewritten);
                        if !cid.is_empty() {
                            pb_updates.push((cid, rewritten));
                        }
                        *raw_val = serde_json::json!({ "__b64__": new_b64 });
                    }
                }
            }
        }
        if !bundle.rows.is_empty() {
            restored_conv_count = bundle.rows.len();
        }

        let apply_req = serde_json::json!({
            "agy_dir": clean_path_string(&agy_dir),
            "rows": bundle.rows,
            "parents": bundle.parents,
            "rewritten_blobs": rewritten_blobs,
        });
        let _ = run_python_script(APPLY_LINK_DB_PYTHON, &apply_req.to_string())?;
        if !pb_updates.is_empty() {
            update_jetbox_summaries_pb(&agy_dir.join("jetbox_summaries_proto.pb"), &pb_updates)
                .map_err(|e| format!("failed updating jetbox_summaries_proto.pb: {e}"))?;
        }

        for cid in &manifest.conversation_ids {
            let src_brain = agy_in.join("brain").join(cid);
            if src_brain.is_dir() {
                copy_dir_filtered(&src_brain, &agy_dir.join("brain").join(cid), Some(&text_pairs))?;
            }
            let src_ann = agy_in.join("annotations").join(format!("{cid}.pbtxt"));
            if src_ann.is_file() {
                let dst_ann_dir = agy_dir.join("annotations");
                std::fs::create_dir_all(&dst_ann_dir)
                    .map_err(|e| format!("failed creating {}: {e}", dst_ann_dir.display()))?;
                std::fs::copy(&src_ann, dst_ann_dir.join(format!("{cid}.pbtxt")))
                    .map_err(|e| format!("failed copying annotation {cid}: {e}"))?;
            }
        }
        merge_history_jsonl(
            &agy_in.join("history.jsonl"),
            &agy_dir.join("history.jsonl"),
            &new_cwd_str,
        )?;
    }

    let mut snap = manifest.workspace_snapshot;
    snap.identity_cwd = workspace_cwd.to_path_buf();
    if let Some(ws_space) = &mut snap.worktree_space {
        ws_space.repo_root = remap_path_with_prefix(&ws_space.repo_root, old_cwd_path, workspace_cwd);
        ws_space.checkout_path =
            remap_path_with_prefix(&ws_space.checkout_path, old_cwd_path, workspace_cwd);
    }

    let mut has_any_agy_session = false;
    for tab in &mut snap.tabs {
        for pane in tab.panes.values_mut() {
            pane.cwd = remap_path_with_prefix(&pane.cwd, old_cwd_path, workspace_cwd);
            if pane
                .agent_session
                .as_ref()
                .is_some_and(|s| s.agent == "agy" && !s.value.is_empty())
            {
                has_any_agy_session = true;
            }
        }
    }

    if !has_any_agy_session {
        if let Some(first_conv) = manifest.conversation_ids.first() {
            let target_tab_idx = if snap.active_tab < snap.tabs.len() {
                snap.active_tab
            } else {
                0
            };
            if let Some(tab) = snap.tabs.get_mut(target_tab_idx) {
                let target_pane = tab.focused.or(tab.root_pane).or_else(|| tab.panes.keys().next().copied());
                if let Some(pid) = target_pane {
                    if let Some(pane) = tab.panes.get_mut(&pid) {
                        pane.agent_name = Some("agy".to_string());
                        pane.managed_agent_kind = Some("antigravity_cli".to_string());
                        pane.agent_session = Some(PaneAgentSessionSnapshot {
                            source: "herdr:antigravity_cli".to_string(),
                            agent: "agy".to_string(),
                            kind: AgentSessionRefKind::Id,
                            value: first_conv.clone(),
                        });
                    }
                }
            }
        }
    }

    Ok((
        snap,
        HerdrLinkSummary {
            link_dir,
            workspace_cwd: workspace_cwd.to_path_buf(),
            conversation_count: restored_conv_count,
            compressed_db_count: db_count,
        },
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::persist::{LayoutSnapshot, PaneSnapshot, TabSnapshot};
    use std::collections::HashMap;

    fn unique_tmp(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "herdr-link-test-{}-{}",
            label,
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create temp test dir");
        dir
    }

    #[test]
    fn rewrites_nested_protobuf_uri_and_updates_varint_lengths() {
        let old_uri = b"file:///C:/Users/Usuario/Documents/GitHub/VeryLongProjectName";
        let new_uri = b"file:///D:/Work/Proj";

        // Build inner msg: field 1 = old_uri, field 2 = 42
        let mut inner = Vec::new();
        inner.push(0x0a);
        write_varint(old_uri.len() as u64, &mut inner);
        inner.extend_from_slice(old_uri);
        inner.push(0x10);
        inner.push(42);

        // Outer msg: field 1 = "chat title", field 9 (0x4a) = inner
        let mut outer = Vec::new();
        outer.extend_from_slice(b"\x0a\x04chat");
        outer.push(0x4a);
        write_varint(inner.len() as u64, &mut outer);
        outer.extend_from_slice(&inner);

        let rewritten = rewrite_protobuf_bytes(&outer, &[(old_uri.as_slice(), new_uri.as_slice())]);
        assert!(contains_subslice(&rewritten, new_uri));
        assert!(!contains_subslice(&rewritten, old_uri));

        // Verify valid protobuf parse cleanly reaches exact end and preserves field 1 and varint 42
        let mut pos = 0usize;
        let (t1, p1) = read_varint(&rewritten, pos).unwrap();
        assert_eq!(t1, 0x0a);
        let (l1, p1b) = read_varint(&rewritten, p1).unwrap();
        assert_eq!(&rewritten[p1b..p1b + l1 as usize], b"chat");
        pos = p1b + l1 as usize;

        let (t9, p9) = read_varint(&rewritten, pos).unwrap();
        assert_eq!(t9, 0x4a);
        let (l9, p9b) = read_varint(&rewritten, p9).unwrap();
        assert_eq!(p9b + l9 as usize, rewritten.len());
        let nested = &rewritten[p9b..];
        let (it1, ip1) = read_varint(nested, 0).unwrap();
        assert_eq!(it1, 0x0a);
        let (il1, ip1b) = read_varint(nested, ip1).unwrap();
        assert_eq!(&nested[ip1b..ip1b + il1 as usize], new_uri);
    }

    #[test]
    fn save_and_link_migrates_workspace_and_agy_sqlite_with_relocation() {
        let root = unique_tmp("roundtrip");
        let pc1_proj = root.join("pc1").join("MyApp");
        let pc1_agy = root.join("pc1").join("agy");
        let pc2_proj = root.join("pc2").join("MyApp");
        let pc2_agy = root.join("pc2").join("agy");

        std::fs::create_dir_all(&pc1_proj).unwrap();
        std::fs::create_dir_all(pc1_agy.join("conversations")).unwrap();
        std::fs::create_dir_all(pc1_agy.join("brain").join("conv-123")).unwrap();
        std::fs::create_dir_all(pc1_agy.join("annotations")).unwrap();
        std::fs::create_dir_all(&pc2_proj).unwrap();

        let pc1_uri = path_to_file_uri(&pc1_proj);
        std::fs::write(
            pc1_agy.join("brain").join("conv-123").join("notes.md"),
            format!("Project at {}", clean_path_string(&pc1_proj)),
        )
        .unwrap();
        std::fs::write(
            pc1_agy.join("annotations").join("conv-123.pbtxt"),
            "annotation_test: true\n",
        )
        .unwrap();
        std::fs::write(
            pc1_agy.join("history.jsonl"),
            serde_json::json!({
                "display": "hola",
                "timestamp": 123456u64,
                "workspace": clean_path_string(&pc1_proj),
                "conversationId": "conv-123"
            })
            .to_string()
                + "\n",
        )
        .unwrap();

        let seed_script = r#"
import json, pathlib, sqlite3, sys
cfg = json.loads(sys.stdin.read())
agy = pathlib.Path(cfg["agy"])
uri = cfg["uri"]
raw_summary = b"\x0a\x04Test\x4a" + bytes([len(uri) + 2, 0x0a, len(uri)]) + uri.encode()
c1 = sqlite3.connect(str(agy / "conversation_summaries.db"))
c1.execute("""CREATE TABLE conversation_summaries (
    conversation_id text PRIMARY KEY, title text NOT NULL DEFAULT "",
    preview text NOT NULL DEFAULT "", step_count integer NOT NULL DEFAULT 0,
    last_modified_time datetime NOT NULL, workspace_uris text NOT NULL,
    status text NOT NULL DEFAULT "", source text NOT NULL DEFAULT "",
    project_id text NOT NULL DEFAULT "", agent_name text NOT NULL DEFAULT "",
    parent_conversation_id text NOT NULL DEFAULT "", nesting_depth integer NOT NULL DEFAULT 0,
    battle_id text NOT NULL DEFAULT "", winning_conversation_id text NOT NULL DEFAULT "",
    not_fully_idle numeric NOT NULL DEFAULT false, killed numeric NOT NULL DEFAULT false,
    last_user_input_time datetime NOT NULL, last_user_input_step_index integer NOT NULL DEFAULT -1,
    app_data_dir text NOT NULL DEFAULT "", raw_summary BLOB, group_id TEXT NOT NULL DEFAULT ''
)""")
c1.execute("CREATE TABLE conversation_parents (conversation_id TEXT PRIMARY KEY, parent_id TEXT NOT NULL) WITHOUT ROWID")
c1.execute(
    "INSERT INTO conversation_summaries (conversation_id, title, last_modified_time, workspace_uris, last_user_input_time, raw_summary) VALUES (?, ?, ?, ?, ?, ?)",
    ("conv-123", "Test", "2026-11-15T12:00:00Z", json.dumps([uri]), "2026-11-15T12:00:00Z", raw_summary),
)
c1.commit()
c1.close()

c2 = sqlite3.connect(str(agy / "conversations" / "conv-123.db"))
c2.execute("CREATE TABLE trajectory_metadata_blob (id text PRIMARY KEY, data blob)")
c2.execute("INSERT INTO trajectory_metadata_blob VALUES ('main', ?)", (b"\x0a" + bytes([len(uri)]) + uri.encode(),))
c2.commit()
c2.close()
"#;
        run_python_script(
            seed_script,
            &serde_json::json!({ "agy": clean_path_string(&pc1_agy), "uri": pc1_uri }).to_string(),
        )
        .unwrap();

        let mut panes = HashMap::new();
        panes.insert(
            0,
            PaneSnapshot {
                cwd: pc1_proj.join("src"),
                label: None,
                agent_name: Some("agy".into()),
                managed_agent_kind: Some("antigravity_cli".into()),
                agent_session: Some(PaneAgentSessionSnapshot {
                    source: "herdr:antigravity_cli".into(),
                    agent: "agy".into(),
                    kind: AgentSessionRefKind::Id,
                    value: "conv-123".into(),
                }),
                launch_argv: None,
            },
        );
        let snap = WorkspaceSnapshot {
            id: None,
            custom_name: Some("MyApp".into()),
            identity_cwd: pc1_proj.clone(),
            worktree_space: None,
            public_pane_numbers: HashMap::new(),
            next_public_pane_number: 2,
            public_tab_numbers: vec![1],
            next_public_tab_number: 2,
            tabs: vec![TabSnapshot {
                custom_name: None,
                layout: LayoutSnapshot::Pane(0),
                panes,
                zoomed: false,
                focused: Some(0),
                root_pane: Some(0),
            }],
            active_tab: 0,
        };

        let save_res = save_herdr_link(&pc1_proj, snap, Some(&pc1_agy)).expect("save_herdr_link");
        assert_eq!(save_res.conversation_count, 1);
        assert_eq!(save_res.compressed_db_count, 1);
        assert!(has_herdr_link(&pc1_proj));

        // Simulate git push/pull: copy herdrLink directory from pc1_proj to pc2_proj
        copy_dir_filtered(&pc1_proj.join(HERDR_LINK_DIR), &pc2_proj.join(HERDR_LINK_DIR), None)
            .unwrap();
        assert!(has_herdr_link(&pc2_proj));

        let (restored_snap, link_res) =
            load_and_link_herdr_link(&pc2_proj, Some(&pc2_agy)).expect("load_and_link");
        assert_eq!(link_res.conversation_count, 1);
        assert_eq!(restored_snap.identity_cwd, pc2_proj);
        assert_eq!(restored_snap.tabs[0].panes[&0].cwd, pc2_proj.join("src"));
        assert!(pc2_agy.join("conversations").join("conv-123.db").is_file());
        assert!(pc2_agy
            .join("annotations")
            .join("conv-123.pbtxt")
            .is_file());

        let copied_md =
            std::fs::read_to_string(pc2_agy.join("brain").join("conv-123").join("notes.md"))
                .unwrap();
        assert!(copied_md.contains(&clean_path_string(&pc2_proj)));

        let verify_script = r#"
import json, pathlib, sqlite3, sys
agy = pathlib.Path(sys.stdin.read())
c1 = sqlite3.connect(str(agy / "conversation_summaries.db"))
row = c1.execute("SELECT workspace_uris, raw_summary FROM conversation_summaries WHERE conversation_id='conv-123'").fetchone()
c2 = sqlite3.connect(str(agy / "conversations" / "conv-123.db"))
blob = c2.execute("SELECT data FROM trajectory_metadata_blob WHERE id='main'").fetchone()[0]
print(json.dumps({
    "uris": row[0],
    "raw_has_pc2": b"pc2/MyApp" in row[1] or b"pc2\\MyApp" in row[1],
    "blob_has_pc2": b"pc2/MyApp" in blob,
}))
"#;
        let v_out = run_python_script(verify_script, &clean_path_string(&pc2_agy)).unwrap();
        let v_json: serde_json::Value = serde_json::from_str(&v_out).unwrap();
        assert!(v_json["uris"].as_str().unwrap().contains("pc2"));
        assert_eq!(v_json["raw_has_pc2"], true);
        assert_eq!(v_json["blob_has_pc2"], true);

        let _ = std::fs::remove_dir_all(&root);
    }
}
