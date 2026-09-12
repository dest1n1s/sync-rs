//! Differential fuzzing: random trees, ignore files, nested repositories and tracked files,
//! synced through the real pipeline and compared with the `check-ignore` oracle.
//! `SYNC_RS_FUZZ_CASES` and `SYNC_RS_FUZZ_SEED` scale and reseed the run.

mod common;

use std::fs;
use std::path::{Path, PathBuf};

use common::{check, commit_all, git, write};
use tempfile::tempdir;

const FILES: &[&str] = &[
    "a",
    "b",
    "keep",
    "x.log",
    "y.log",
    "keep.log",
    "z.tmp",
    "st*ar",
    "qu?",
    "br[k]",
    "sp ace",
    "#hash",
    "-dash",
    "new\nline",
    "deep",
    "out",
    "ü.log",
    "trail ",
];

const DIRS: &[&str] = &[
    "a",
    "b",
    "build",
    "out",
    "sub",
    "deep",
    "d*ir",
    "sp ace",
    "vendor",
    "node_modules",
];

const PATTERNS: &[&str] = &[
    "*.log",
    "!keep.log",
    "build/",
    "/build",
    "out",
    "/a",
    "a/b",
    "**/deep",
    "deep/**",
    "*",
    "!keep",
    "!*.log",
    "?",
    "[ab]",
    "[!a]*",
    "st\\*ar",
    "qu\\?",
    "\\#hash",
    "sp ace",
    "sub/",
    "!/sub/keep",
    "*.tmp",
    "-dash",
    "/x.log",
    "**/out/*.log",
    "!out/",
    "b",
    "!b",
    "d\\*ir/",
    "vendor/",
    "!vendor/",
    "*/",
    "**",
    "a/**/b",
    "trail\\ ",
    "ü.log",
    "# comment",
    "",
    "node_modules",
    "!/node_modules/keep",
];

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }

    fn chance(&mut self, p: f64) -> bool {
        (self.next() % 1000) as f64 / 1000.0 < p
    }

    fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[self.below(items.len())]
    }
}

struct Gen {
    rng: Rng,
    root: PathBuf,
    log: String,
    dirs: Vec<PathBuf>,
    files: Vec<PathBuf>,
}

impl Gen {
    fn note(&mut self, action: &str, path: &Path) {
        let rel = path.strip_prefix(&self.root).unwrap_or(path);
        self.log.push_str(&format!("{action} {rel:?}\n"));
    }

    fn file(&mut self, path: PathBuf) {
        if path.exists() {
            return;
        }
        write(&path, "");
        self.note("file", &path);
        self.files.push(path);
    }

    fn gitignore(&mut self, dir: &Path, max_lines: usize) {
        let lines: Vec<&str> = (0..1 + self.rng.below(max_lines))
            .map(|_| *self.rng.pick(PATTERNS))
            .collect();
        let contents = lines.join("\n") + "\n";
        write(dir.join(".gitignore"), &contents);
        self.note(&format!("gitignore {lines:?} in"), dir);
    }

    fn grow(&mut self, dir: &Path, depth: usize) {
        for _ in 0..self.rng.below(4) {
            let name = *self.rng.pick(FILES);
            self.file(dir.join(name));
        }
        if self.rng.chance(0.6) {
            self.gitignore(dir, 4);
        }
        if depth < 3 {
            for _ in 0..self.rng.below(3) {
                let sub = dir.join(*self.rng.pick(DIRS));
                if sub.is_file() {
                    continue;
                }
                if !sub.exists() {
                    fs::create_dir(&sub).unwrap();
                    self.note("dir", &sub);
                    self.dirs.push(sub.clone());
                }
                self.grow(&sub, depth + 1);
            }
        }
    }

    fn nested_repo(&mut self, dir: &Path) {
        git(dir, &["init", "--quiet"]);
        self.note("git init", dir);
        if self.rng.chance(0.7) {
            self.gitignore(dir, 3);
        }
        commit_all(dir);
    }
}

fn run_case(seed: u64) {
    let tmp = tempdir().unwrap();
    let root = tmp.path().join("root");
    fs::create_dir(&root).unwrap();
    let mut g = Gen {
        rng: Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1),
        root: root.clone(),
        log: String::new(),
        dirs: Vec::new(),
        files: Vec::new(),
    };

    let root_is_repo = g.rng.chance(0.8);
    if root_is_repo {
        git(&root, &["init", "--quiet"]);
        g.note("git init", &root);
    }
    g.grow(&root, 0);

    // Repositories that exist when the root commits become gitlinks (or stay untracked when
    // ignored); deepest first so each has a commit before an enclosing one adds it.
    let mut embedded: Vec<PathBuf> = Vec::new();
    for _ in 0..g.rng.below(3) {
        if g.dirs.is_empty() {
            break;
        }
        let dir = g.rng.pick(&g.dirs).clone();
        if !embedded.contains(&dir) && !dir.join(".git").exists() {
            embedded.push(dir);
        }
    }
    embedded.sort_by_key(|dir| std::cmp::Reverse(dir.components().count()));
    for dir in &embedded {
        g.nested_repo(dir);
    }

    if root_is_repo {
        if g.rng.chance(0.3) {
            let lines: Vec<&str> = (0..1 + g.rng.below(2))
                .map(|_| *g.rng.pick(PATTERNS))
                .collect();
            write(root.join(".git/info/exclude"), &(lines.join("\n") + "\n"));
            g.log.push_str(&format!("info/exclude {lines:?}\n"));
        }
        commit_all(&root);
        for _ in 0..g.rng.below(3) {
            if g.files.is_empty() {
                break;
            }
            let file = g.rng.pick(&g.files).clone();
            if embedded.iter().any(|dir| file.starts_with(dir)) {
                continue;
            }
            git(&root, &["add", "--force", file.to_str().unwrap()]);
            g.note("git add --force", &file);
        }
        commit_all(&root);
    }

    // Repositories cloned into fresh directories after the root committed stay untracked.
    for _ in 0..g.rng.below(3) {
        let parent = if g.dirs.is_empty() || g.rng.chance(0.3) {
            root.clone()
        } else {
            g.rng.pick(&g.dirs).clone()
        };
        let dir = parent.join(*g.rng.pick(DIRS));
        if dir.exists() {
            continue;
        }
        fs::create_dir(&dir).unwrap();
        g.note("dir", &dir);
        g.dirs.push(dir.clone());
        g.grow(&dir, 2);
        g.nested_repo(&dir);
    }

    for _ in 0..g.rng.below(4) {
        let dir = if g.dirs.is_empty() || g.rng.chance(0.3) {
            root.clone()
        } else {
            g.rng.pick(&g.dirs).clone()
        };
        let name = *g.rng.pick(FILES);
        g.file(dir.join(name));
    }

    let sync_root = if !g.dirs.is_empty() && g.rng.chance(0.25) {
        g.rng.pick(&g.dirs).clone()
    } else {
        root.clone()
    };
    g.note("sync root", &sync_root);

    check(&sync_root, tmp.path(), &format!("seed {seed}\n{}", g.log));
}

#[test]
fn random_trees_match_the_oracle() {
    let cases: u64 = std::env::var("SYNC_RS_FUZZ_CASES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(25);
    let seed: u64 = std::env::var("SYNC_RS_FUZZ_SEED")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(1);
    for case in seed..seed + cases {
        run_case(case);
    }
}
