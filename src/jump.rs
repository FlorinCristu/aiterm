//! `z`-style jumping: folders visited with `cd` are remembered in ~/.aiterm_dirs, ranked by
//! frecency (how often, weighted by how recently), and `z proj` picks the best match.
//!
//! One line per folder, `<score>\t<last_visit_unix_secs>\t<path>`, so it stays hand-editable.

use std::path::{Path, PathBuf};

const MAX_ENTRIES: usize = 200;
/// Once the scores add up past this, everything decays so old haunts fade out.
const MAX_TOTAL: f64 = 1000.0;
const DECAY: f64 = 0.9;

#[derive(Debug, Clone, PartialEq)]
struct Entry {
    score: f64,
    last: u64,
    path: String,
}

fn file() -> Option<PathBuf> {
    crate::ai::home_dir().map(|h| h.join(".aiterm_dirs"))
}

fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

fn load() -> Vec<Entry> {
    file().and_then(|p| std::fs::read_to_string(p).ok()).map(|s| parse(&s)).unwrap_or_default()
}

/// Remember a visit to `dir` (called after every successful cd). Cheap; errors are ignored.
pub fn record(dir: &Path) {
    let home = crate::ai::home_dir();
    if dir.parent().is_none() || home.is_some_and(|h| key(&h.to_string_lossy()) == key(&dir.to_string_lossy())) {
        return;
    }
    let Some(path) = dir.to_str().filter(|s| !s.contains(['\n', '\r'])) else { return };
    let Some(f) = file() else { return };
    let mut entries = load();
    add(&mut entries, path, now());
    // Temp + rename so a crash mid-write never leaves a half file behind.
    let tmp = f.with_extension("tmp");
    if std::fs::write(&tmp, serialize(&entries)).is_ok() {
        let _ = std::fs::rename(&tmp, &f);
    }
}

/// The best-ranked remembered folder matching `query`, skipping `cwd` itself and folders that no longer exist.
pub fn find(query: &str, cwd: &Path) -> Option<PathBuf> {
    top(query, cwd, 1).pop()
}

/// Up to `n` best matches for `query` (empty query = top folders), best first — for a `z` with no argument listing.
pub fn top(query: &str, cwd: &Path, n: usize) -> Vec<PathBuf> {
    best(&load(), query, &cwd.to_string_lossy(), now(), &|p| Path::new(p).is_dir(), n)
        .into_iter()
        .map(PathBuf::from)
        .collect()
}

/// Comparable form of a path: forward slashes, no trailing slash, case-folded where the filesystem is.
fn key(p: &str) -> String {
    let s = p.replace('\\', "/");
    let s = if s.len() > 1 { s.trim_end_matches('/') } else { &s };
    if cfg!(windows) { s.to_lowercase() } else { s.to_string() }
}

fn parse(s: &str) -> Vec<Entry> {
    s.lines()
        .filter_map(|l| {
            let mut f = l.splitn(3, '\t');
            let score = f.next()?.trim().parse::<f64>().ok().filter(|s| s.is_finite() && *s > 0.0)?;
            let last = f.next()?.trim().parse().ok()?;
            let path = f.next()?.trim().to_string();
            (!path.is_empty()).then_some(Entry { score, last, path })
        })
        .collect()
}

fn serialize(entries: &[Entry]) -> String {
    entries.iter().map(|e| format!("{:.2}\t{}\t{}\n", e.score, e.last, e.path)).collect()
}

fn rank(e: &Entry, now: u64) -> f64 {
    let age = now.saturating_sub(e.last);
    let factor = match age {
        0..=3_599 => 4.0,
        3_600..=86_399 => 2.0,
        86_400..=604_799 => 0.5,
        _ => 0.25,
    };
    e.score * factor
}

/// Count a visit, decaying and trimming so the file stays small. Decay comes first so the
/// visit being counted is never the one aged away.
fn add(entries: &mut Vec<Entry>, path: &str, now: u64) {
    if entries.iter().map(|e| e.score).sum::<f64>() + 1.0 > MAX_TOTAL {
        entries.iter_mut().for_each(|e| e.score *= DECAY);
        entries.retain(|e| e.score >= 1.0);
    }
    let k = key(path);
    match entries.iter_mut().find(|e| key(&e.path) == k) {
        Some(e) => {
            e.score += 1.0;
            e.last = now;
            e.path = path.to_string();
        }
        None => entries.push(Entry { score: 1.0, last: now, path: path.to_string() }),
    }
    entries.sort_by(|a, b| rank(b, now).total_cmp(&rank(a, now)));
    entries.truncate(MAX_ENTRIES);
}

/// zoxide's rule: terms appear in order, and the last one inside the last path component.
/// Returns whether that last component is exactly the last term, for tie-breaking.
fn matches(path: &str, terms: &[String]) -> Option<bool> {
    let p = path.replace('\\', "/").to_lowercase();
    let p = p.trim_end_matches('/');
    let comp_start = p.rfind('/').map_or(0, |i| i + 1);
    let Some((last, rest)) = terms.split_last() else { return Some(false) };
    let mut pos = 0;
    for t in rest {
        pos += p[pos..].find(t.as_str())? + t.len();
    }
    let from = pos.max(comp_start);
    p[from..].find(last.as_str())?;
    Some(p[comp_start..] == **last)
}

fn best(entries: &[Entry], query: &str, cwd: &str, now: u64, exists: &dyn Fn(&str) -> bool, n: usize) -> Vec<String> {
    let terms: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
    let cwd = key(cwd);
    let mut hits: Vec<(f64, bool, &str)> = entries
        .iter()
        .filter(|e| key(&e.path) != cwd)
        .filter_map(|e| Some((rank(e, now), matches(&e.path, &terms)?, e.path.as_str())))
        .collect();
    hits.sort_by(|a, b| b.0.total_cmp(&a.0).then(b.1.cmp(&a.1)));
    hits.into_iter().filter(|h| exists(h.2)).take(n).map(|h| h.2.to_string()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: u64 = 1_000_000_000;

    fn e(score: f64, ago: u64, path: &str) -> Entry {
        Entry { score, last: NOW - ago, path: path.into() }
    }

    fn terms(q: &str) -> Vec<String> {
        q.split_whitespace().map(str::to_lowercase).collect()
    }

    #[test]
    fn parses_and_round_trips_tolerating_junk() {
        let s = "3.50\t100\t/home/a/proj\ngarbage\n-1\t5\t/x\nnan\t5\t/y\n2\tx\t/z\n1\t7\t/with\ttab\n";
        let v = parse(s);
        assert_eq!(v, vec![Entry { score: 3.5, last: 100, path: "/home/a/proj".into() }, Entry { score: 1.0, last: 7, path: "/with\ttab".into() }]);
        assert_eq!(parse(&serialize(&v)), v);
    }

    #[test]
    fn recency_weights_rank() {
        assert_eq!(rank(&e(1.0, 10, "/a"), NOW), 4.0);
        assert_eq!(rank(&e(1.0, 7_200, "/a"), NOW), 2.0);
        assert_eq!(rank(&e(1.0, 200_000, "/a"), NOW), 0.5);
        assert_eq!(rank(&e(1.0, 10_000_000, "/a"), NOW), 0.25);
    }

    #[test]
    fn add_counts_ages_and_caps() {
        let mut v = vec![];
        add(&mut v, "/a", NOW);
        add(&mut v, "/a/", NOW);
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].score, 2.0);

        let mut v = vec![e(999.5, 0, "/big"), e(1.05, 0, "/small")];
        add(&mut v, "/new", NOW);
        assert!(v.iter().any(|e| e.path == "/big" && (e.score - 899.55).abs() < 1e-9));
        assert!(v.iter().any(|e| e.path == "/new" && e.score == 1.0));
        assert!(!v.iter().any(|e| e.path == "/small"));

        let mut v: Vec<Entry> = (0..MAX_ENTRIES).map(|i| e(1.0, 10_000_000, &format!("/old{i}"))).collect();
        add(&mut v, "/fresh", NOW);
        assert_eq!(v.len(), MAX_ENTRIES);
        assert_eq!(v[0].path, "/fresh");
    }

    #[test]
    fn matching_follows_zoxide_rules() {
        assert_eq!(matches("/home/a/proj", &terms("proj")), Some(true));
        assert_eq!(matches("/home/a/myproject", &terms("PROJ")), Some(false));
        assert_eq!(matches("/home/proj/src", &terms("proj")), None);
        assert_eq!(matches("/home/proj/src", &terms("proj src")), Some(true));
        assert_eq!(matches("/home/proj/src", &terms("src proj")), None);
        assert_eq!(matches(r"C:\Users\a\Code\aiterm", &terms("code ait")), Some(false));
        assert_eq!(matches("/a/b", &[]), Some(false));
    }

    #[test]
    fn best_ranks_skips_cwd_and_missing() {
        let v = vec![e(10.0, 10, "/w/proj-old"), e(5.0, 10, "/w/proj"), e(1.0, 10, "/w/projx"), e(20.0, 10, "/gone/proj")];
        let exists = |p: &str| !p.starts_with("/gone");
        assert_eq!(best(&v, "proj", "/", NOW, &exists, 3), vec!["/w/proj-old", "/w/proj", "/w/projx"]);
        assert_eq!(best(&v, "proj", "/w/proj-old", NOW, &exists, 1), vec!["/w/proj"]);
        assert_eq!(best(&v, "", "/", NOW, &exists, 2), vec!["/w/proj-old", "/w/proj"]);
        // Same rank: the exact name wins over a substring.
        let tie = vec![e(2.0, 10, "/w/projx"), e(2.0, 10, "/w/proj")];
        assert_eq!(best(&tie, "proj", "/", NOW, &|_| true, 1), vec!["/w/proj"]);
    }

    #[test]
    fn real_folders_are_found() {
        let root = std::env::temp_dir().join(format!("aiterm_jump_{}", std::process::id()));
        let dir = root.join("Projects").join("zjumptarget");
        std::fs::create_dir_all(&dir).unwrap();
        let v = vec![e(1.0, 10, dir.to_str().unwrap()), e(9.0, 10, root.join("missing_zjump").to_str().unwrap())];
        let exists = |p: &str| Path::new(p).is_dir();
        assert_eq!(best(&v, "proj zjump", "/", NOW, &exists, 5), vec![dir.to_str().unwrap().to_string()]);
        let _ = std::fs::remove_dir_all(&root);
    }
}
