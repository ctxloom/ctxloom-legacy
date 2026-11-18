//! git's output read: its version, an ignore rule, `status --porcelain=v2
//! -z`, and `diff-tree -z`'s raw records. Pure functions of the bytes, so
//! each form is tested on what git prints.

use ctxloom_ports::{IgnoreRule, PathStatus, StatusEntry, TreeChange};

/// The version word of `git --version`'s line (`2.43.0` of `git version
/// 2.43.0`, `2.39.3` of `git version 2.39.3 (Apple Git-145)`); the whole
/// line when it has no such word.
pub(crate) fn version_word(said: &str) -> &str {
    said.strip_prefix("git version ")
        .and_then(|rest| rest.split_whitespace().next())
        .unwrap_or(said)
}

/// The major and minor version of `git --version`'s line; none when it does
/// not say them (a build that words it otherwise is not judged).
pub(crate) fn version(said: &str) -> Option<(u32, u32)> {
    let word = said.strip_prefix("git version ")?;
    let mut parts = word.split(|c: char| !c.is_ascii_digit());
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    Some((major, minor))
}

/// The NUL-terminated fields of `-z` output.
fn fields(out: &[u8]) -> Vec<String> {
    let mut fields: Vec<String> = out
        .split(|b| *b == 0)
        .map(|f| String::from_utf8_lossy(f).into_owned())
        .collect();
    // The last field is terminated too: what follows it is no field.
    if fields.last().is_some_and(String::is_empty) {
        fields.pop();
    }
    fields
}

/// `check-ignore -v -z`'s record (`source, line, pattern, path`): the rule,
/// none when there is no record or the rule that matched is a negation (`!`),
/// which un-ignores.
pub(crate) fn ignore_rule(out: &[u8]) -> Option<IgnoreRule> {
    let fields = fields(out);
    let [source, line, pattern, ..] = fields.as_slice() else {
        return None;
    };
    if pattern.is_empty() || pattern.starts_with('!') {
        return None;
    }
    Some(IgnoreRule {
        source: source.clone(),
        line: line.parse().ok()?,
        pattern: pattern.clone(),
    })
}

/// `path`, relative to the repository's root, made relative to the
/// directory `prefix` names (`.ctxloom/`); none outside it.
fn below(path: &str, prefix: &str) -> Option<String> {
    path.strip_prefix(prefix).map(str::to_string)
}

/// The status of a changed (`1`) entry by its `XY`: what the path is beside
/// the commit, staged or not.
fn changed(xy: &str) -> PathStatus {
    let mut letters = xy.chars();
    let x = letters.next().unwrap_or('.');
    let y = letters.next().unwrap_or('.');
    if x == 'D' || y == 'D' {
        PathStatus::Deleted
    } else if x == 'A' {
        PathStatus::Added
    } else if x == 'T' || y == 'T' {
        PathStatus::TypeChanged
    } else {
        PathStatus::Modified
    }
}

/// `status --porcelain=v2 -z`'s entries under `prefix`, each path relative
/// to it, sorted by path.
pub(crate) fn status(out: &[u8], prefix: &str) -> Vec<StatusEntry> {
    let fields = fields(out);
    let mut entries = Vec::new();
    let mut at = 0;
    while at < fields.len() {
        let record = &fields[at];
        at += 1;
        let (kind, rest) = record.split_once(' ').unwrap_or((record.as_str(), ""));
        let entry = match kind {
            // `1 XY sub mH mI mW hH hI path`
            "1" => rest
                .splitn(8, ' ')
                .collect::<Vec<_>>()
                .split_last()
                .and_then(|(path, head)| {
                    Some(StatusEntry {
                        path: below(path, prefix)?,
                        status: changed(head.first()?),
                        from_path: None,
                        untracked: false,
                    })
                }),
            // `2 XY sub mH mI mW hH hI Xscore path`, then the path it was.
            "2" => {
                let from = fields.get(at).cloned().unwrap_or_default();
                at += 1;
                rest.splitn(9, ' ').last().and_then(|path| {
                    Some(StatusEntry {
                        path: below(path, prefix)?,
                        status: PathStatus::Renamed,
                        from_path: below(&from, prefix),
                        untracked: false,
                    })
                })
            }
            // `u XY sub m1 m2 m3 mW h1 h2 h3 path`
            "u" => rest.splitn(10, ' ').last().and_then(|path| {
                Some(StatusEntry {
                    path: below(path, prefix)?,
                    status: PathStatus::Unmerged,
                    from_path: None,
                    untracked: false,
                })
            }),
            "?" => below(rest, prefix).map(|path| StatusEntry {
                path,
                status: PathStatus::Added,
                from_path: None,
                untracked: true,
            }),
            // Headers (`#`) and ignored entries (`!`) are not asked for.
            _ => None,
        };
        entries.extend(entry);
    }
    entries.sort_by(|a, b| a.path.cmp(&b.path));
    entries
}

/// An object id that names nothing (all zeros) is none.
fn oid(id: &str) -> Option<String> {
    (!id.is_empty() && !id.bytes().all(|b| b == b'0')).then(|| id.to_string())
}

/// `diff-tree -r -M -z --relative`'s raw records (`:mode mode src dst
/// STATUS`, then the path, or the path it was and the path it is), sorted
/// by path.
pub(crate) fn diff_tree(out: &[u8]) -> Vec<TreeChange> {
    let fields = fields(out);
    let mut changes = Vec::new();
    let mut at = 0;
    while at < fields.len() {
        let record = &fields[at];
        at += 1;
        let Some(meta) = record.strip_prefix(':') else {
            continue;
        };
        let parts: Vec<&str> = meta.split(' ').collect();
        let [_, _, src, dst, letter, ..] = parts.as_slice() else {
            continue;
        };
        let renamed = letter.starts_with('R') || letter.starts_with('C');
        let (from_path, path) = if renamed {
            let from = fields.get(at).cloned();
            let to = fields.get(at + 1).cloned();
            at += 2;
            (from, to)
        } else {
            let path = fields.get(at).cloned();
            at += 1;
            (None, path)
        };
        let Some(path) = path else {
            break;
        };
        let status = match letter.chars().next() {
            Some('A') | Some('C') => PathStatus::Added,
            Some('D') => PathStatus::Deleted,
            Some('R') => PathStatus::Renamed,
            Some('T') => PathStatus::TypeChanged,
            Some('U') => PathStatus::Unmerged,
            _ => PathStatus::Modified,
        };
        changes.push(TreeChange {
            path,
            status,
            from_path: from_path.filter(|_| letter.starts_with('R')),
            from_oid: oid(src),
            to_oid: oid(dst),
        });
    }
    changes.sort_by(|a, b| a.path.cmp(&b.path));
    changes
}

#[cfg(test)]
#[path = "parse.test.rs"]
mod tests;
