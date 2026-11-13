use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const PERIODIC_REFRESH_INTERVAL: Duration = Duration::from_secs(2);
const MAX_TREE_DEPTH: usize = 12;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileTreeEntry {
    pub path: PathBuf,
    pub name: String,
    pub is_dir: bool,
    pub is_parent_link: bool,
    pub depth: usize,
    pub is_expanded: bool,
}

#[derive(Debug, Clone, Default)]
pub struct FileTreeState {
    pub root_dir: Option<PathBuf>,
    pub followed_pane_cwd: Option<PathBuf>,
    pub expanded_dirs: HashSet<PathBuf>,
    pub entries: Vec<FileTreeEntry>,
    pub selected_path: Option<PathBuf>,
    pub scroll: usize,
    pub focused: bool,
    pub last_refresh: Option<Instant>,
    pub last_click: Option<(PathBuf, Instant)>,
}

impl FileTreeState {
    pub fn sync_pane_cwd(&mut self, cwd: Option<PathBuf>) {
        if self.followed_pane_cwd != cwd {
            self.followed_pane_cwd = cwd.clone();
            self.root_dir = cwd;
            self.scroll = 0;
            self.selected_path = None;
            self.refresh();
        }
    }

    pub fn maybe_periodic_refresh(&mut self) {
        if self.root_dir.is_none() {
            return;
        }
        if self
            .last_refresh
            .is_none_or(|t| t.elapsed() >= PERIODIC_REFRESH_INTERVAL)
        {
            self.refresh();
        }
    }

    pub fn refresh(&mut self) {
        self.entries.clear();
        if let Some(root) = self.root_dir.clone() {
            if let Some(parent) = root.parent() {
                self.entries.push(FileTreeEntry {
                    path: parent.to_path_buf(),
                    name: "..".to_string(),
                    is_dir: true,
                    is_parent_link: true,
                    depth: 0,
                    is_expanded: false,
                });
            }
            let mut visited = HashSet::new();
            visited.insert(root.clone());
            Self::collect_dir_entries(
                &root,
                0,
                &self.expanded_dirs,
                &mut visited,
                &mut self.entries,
            );
        }

        if let Some(selected) = self.selected_path.as_ref() {
            if !self.entries.iter().any(|entry| &entry.path == selected) {
                self.selected_path = None;
            }
        }
        if self.entries.is_empty() {
            self.scroll = 0;
        } else if self.scroll >= self.entries.len() {
            self.scroll = self.entries.len() - 1;
        }
        self.last_refresh = Some(Instant::now());
    }

    fn collect_dir_entries(
        dir: &Path,
        depth: usize,
        expanded_dirs: &HashSet<PathBuf>,
        visited: &mut HashSet<PathBuf>,
        out: &mut Vec<FileTreeEntry>,
    ) {
        if depth >= MAX_TREE_DEPTH {
            return;
        }
        let Ok(read_dir) = std::fs::read_dir(dir) else {
            return;
        };

        let mut children: Vec<(PathBuf, String, bool, bool)> = read_dir
            .flatten()
            .filter_map(|entry| {
                let path = entry.path();
                let name = entry.file_name().to_string_lossy().into_owned();
                let file_type = entry.file_type().ok();
                let is_symlink = file_type.as_ref().is_some_and(|ft| ft.is_symlink());
                let is_dir = file_type
                    .as_ref()
                    .map(|ft| {
                        if ft.is_symlink() {
                            path.is_dir()
                        } else {
                            ft.is_dir()
                        }
                    })
                    .unwrap_or_else(|| path.is_dir());
                Some((path, name, is_dir, is_symlink))
            })
            .collect();

        children.sort_by(|a, b| {
            b.2.cmp(&a.2)
                .then_with(|| a.1.to_lowercase().cmp(&b.1.to_lowercase()))
                .then_with(|| a.1.cmp(&b.1))
        });

        for (path, name, is_dir, is_symlink) in children {
            let is_expanded = is_dir && expanded_dirs.contains(&path);
            out.push(FileTreeEntry {
                path: path.clone(),
                name,
                is_dir,
                is_parent_link: false,
                depth,
                is_expanded,
            });
            if is_expanded && !is_symlink && visited.insert(path.clone()) {
                Self::collect_dir_entries(&path, depth + 1, expanded_dirs, visited, out);
            }
        }
    }

    pub fn is_diverged_from_pane_cwd(&self) -> bool {
        self.followed_pane_cwd.is_some() && self.followed_pane_cwd != self.root_dir
    }

    pub fn snap_to_pane_cwd(&mut self) {
        self.root_dir = self.followed_pane_cwd.clone();
        self.scroll = 0;
        self.selected_path = None;
        self.refresh();
    }

    pub fn go_up(&mut self) {
        let Some(current) = self.root_dir.clone() else {
            return;
        };
        let Some(parent) = current.parent() else {
            return;
        };
        self.root_dir = Some(parent.to_path_buf());
        self.scroll = 0;
        self.selected_path = Some(current);
        self.refresh();
        self.ensure_selected_visible(12);
    }

    pub fn enter_dir(&mut self, dir: PathBuf) {
        self.root_dir = Some(dir);
        self.scroll = 0;
        self.selected_path = None;
        self.refresh();
    }

    pub fn toggle_expand(&mut self, dir: &Path) {
        if self.expanded_dirs.contains(dir) {
            self.expanded_dirs.remove(dir);
        } else {
            self.expanded_dirs.insert(dir.to_path_buf());
        }
        self.refresh();
    }

    pub fn expand_selected(&mut self) {
        let Some(entry) = self.selected_entry().cloned() else {
            return;
        };
        if entry.is_parent_link {
            self.go_up();
        } else if entry.is_dir && !entry.is_expanded {
            self.expanded_dirs.insert(entry.path);
            self.refresh();
        }
    }

    pub fn collapse_or_go_up(&mut self) {
        let Some(entry) = self.selected_entry().cloned() else {
            self.go_up();
            return;
        };
        if entry.is_dir && !entry.is_parent_link && entry.is_expanded {
            self.expanded_dirs.remove(&entry.path);
            self.refresh();
            return;
        }
        if let Some(parent) = entry.path.parent() {
            if let Some(root) = self.root_dir.as_ref() {
                if parent != root && parent.starts_with(root) {
                    self.selected_path = Some(parent.to_path_buf());
                    self.expanded_dirs.remove(parent);
                    self.refresh();
                    return;
                }
            }
        }
        self.go_up();
    }

    pub fn selected_index(&self) -> Option<usize> {
        let selected = self.selected_path.as_ref()?;
        self.entries.iter().position(|entry| &entry.path == selected)
    }

    pub fn selected_entry(&self) -> Option<&FileTreeEntry> {
        self.selected_index().and_then(|idx| self.entries.get(idx))
    }

    pub fn move_selection(&mut self, delta: isize, viewport_rows: usize) {
        if self.entries.is_empty() {
            self.selected_path = None;
            return;
        }
        let next_idx = match self.selected_index() {
            Some(current) => (current as isize + delta)
                .clamp(0, self.entries.len().saturating_sub(1) as isize)
                as usize,
            None => {
                if delta >= 0 {
                    0
                } else {
                    self.entries.len().saturating_sub(1)
                }
            }
        };
        self.selected_path = Some(self.entries[next_idx].path.clone());
        self.ensure_selected_visible(viewport_rows);
    }

    pub fn ensure_selected_visible(&mut self, viewport_rows: usize) {
        if viewport_rows == 0 {
            return;
        }
        let Some(idx) = self.selected_index() else {
            return;
        };
        if idx < self.scroll {
            self.scroll = idx;
        } else if idx >= self.scroll + viewport_rows {
            self.scroll = idx + 1 - viewport_rows;
        }
        self.clamp_scroll(viewport_rows);
    }

    pub fn scroll_by(&mut self, delta: isize, viewport_rows: usize) {
        let max_scroll = self.entries.len().saturating_sub(viewport_rows);
        self.scroll = (self.scroll as isize + delta).clamp(0, max_scroll as isize) as usize;
    }

    pub fn clamp_scroll(&mut self, viewport_rows: usize) {
        let max_scroll = self.entries.len().saturating_sub(viewport_rows);
        if self.scroll > max_scroll {
            self.scroll = max_scroll;
        }
    }

    pub fn current_explorer_dir(&self) -> Option<&PathBuf> {
        self.root_dir
            .as_ref()
            .or(self.followed_pane_cwd.as_ref())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unique_temp_dir(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "herdr-file-tree-test-{}-{}",
            label,
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create temp test dir");
        dir
    }

    #[test]
    fn sorts_dirs_before_files_and_expands_lazily() {
        let root = unique_temp_dir("sort-expand");
        let sub = root.join("alpha_dir");
        std::fs::create_dir_all(&sub).expect("create subdir");
        std::fs::write(sub.join("nested.rs"), "fn main() {}").expect("write nested file");
        std::fs::write(root.join("zeta.txt"), "hi").expect("write zeta");
        std::fs::write(root.join("Beta.txt"), "hi").expect("write Beta");

        let mut state = FileTreeState::default();
        state.sync_pane_cwd(Some(root.clone()));

        let visible: Vec<&str> = state.entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(visible, vec!["..", "alpha_dir", "Beta.txt", "zeta.txt"]);

        state.toggle_expand(&sub);
        let expanded: Vec<(&str, usize)> = state
            .entries
            .iter()
            .map(|e| (e.name.as_str(), e.depth))
            .collect();
        assert_eq!(
            expanded,
            vec![
                ("..", 0),
                ("alpha_dir", 0),
                ("nested.rs", 1),
                ("Beta.txt", 0),
                ("zeta.txt", 0),
            ]
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn enter_dir_and_go_up_and_snap_back() {
        let root = unique_temp_dir("nav");
        let sub = root.join("sub");
        std::fs::create_dir_all(&sub).expect("create sub");

        let mut state = FileTreeState::default();
        state.sync_pane_cwd(Some(root.clone()));
        assert!(!state.is_diverged_from_pane_cwd());

        state.enter_dir(sub.clone());
        assert_eq!(state.root_dir.as_ref(), Some(&sub));
        assert!(state.is_diverged_from_pane_cwd());

        state.go_up();
        assert_eq!(state.root_dir.as_ref(), Some(&root));
        assert_eq!(state.selected_path.as_ref(), Some(&sub));
        assert!(!state.is_diverged_from_pane_cwd());

        state.enter_dir(sub.clone());
        state.snap_to_pane_cwd();
        assert_eq!(state.root_dir.as_ref(), Some(&root));

        let _ = std::fs::remove_dir_all(&root);
    }
}
