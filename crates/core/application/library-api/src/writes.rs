//! A library write as the daemon makes it (step 10b U3; protos.md §1.1):
//! the use case PLANS it over recording ports
//! ([`library_application::write::recording`]), and what it planned is
//! shaped here into the one step of the library domain it is: one file
//! replaced, one new directory, or one path into the trash. A plan of more
//! than one file is refused `OneChangePerCall` before any command (T29),
//! in the domain's own words; a plan of nothing writes nothing.
//!
//! What the step's command is (its paths, the ids git would give what it
//! writes) is [`intent_of`]; what the step left, as the re-observe sees
//! it, is [`left_by`].

use std::collections::BTreeMap;

pub use ctxloom_ports::Scope;
use ctxloom_ports::{Change, TreeError, TreeObserver};
use library_application::write::recording::Planned;
use library_tree::{
    CreateDirectory, DirtyPath, Evidence, LibraryTreeRejection, ReplaceFile, TrashPath,
};

/// Where a bundle's directory is in its tree.
pub const BUNDLES: &str = "content/bundles/v2";

/// The config file's path in its tree.
pub const CONFIG: &str = "config.yaml";

/// Which tree a write lands in: the content's (the project's, else the
/// home's) or a config layer's.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lands {
    Content,
    Layer(Scope),
}

/// The one step a write is, its paths relative to its tree's `.ctxloom`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Shape {
    /// One file written whole.
    Replace { path: String, bytes: Vec<u8> },
    /// One new directory and the files in it, each below it.
    Create {
        path: String,
        files: Vec<(String, Vec<u8>)>,
    },
    /// One path taken away (into the trash, U8), with every file under it
    /// (below it; none for a file).
    Trash { path: String, files: Vec<String> },
}

impl Shape {
    /// The path the step changes: what its etag is of.
    pub fn path(&self) -> &str {
        match self {
            Shape::Replace { path, .. }
            | Shape::Create { path, .. }
            | Shape::Trash { path, .. } => path,
        }
    }
}

/// A planned write, shaped: the tree it lands in, its step, and what to
/// perform for it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Shaped {
    pub lands: Lands,
    pub shape: Shape,
    pub planned: Planned,
}

/// A bundle's directory in its tree.
pub fn bundle_path(bundle: &str) -> String {
    format!("{BUNDLES}/{bundle}")
}

/// What `planned` (a use case's recorded writes, in order) is: none when it
/// writes nothing; refused `OneChangePerCall` when it changes more than one
/// file, each named below its bundle. `files_under` answers the files
/// below a directory of a bundle now (`""`: the bundle's own), each below
/// it: several files written into a directory with none are one new
/// directory (a skill).
pub fn shape(
    planned: Vec<Planned>,
    files_under: impl Fn(&str, &str) -> Vec<String>,
) -> Result<Option<Shaped>, LibraryTreeRejection> {
    let mut planned = planned;
    let one = match planned.len() {
        0 => return Ok(None),
        1 => planned.remove(0),
        _ => {
            return Err(LibraryTreeRejection::OneChangePerCall {
                paths: planned.iter().flat_map(paths_of).collect(),
            })
        }
    };
    let shaped = |lands, shape| {
        Ok(Some(Shaped {
            lands,
            shape,
            planned: one.clone(),
        }))
    };
    match &one {
        Planned::Layer { scope, text } => shaped(
            Lands::Layer(*scope),
            Shape::Replace {
                path: CONFIG.to_string(),
                bytes: text.as_bytes().to_vec(),
            },
        ),
        Planned::NewBundle { bundle, files } => shaped(
            Lands::Content,
            Shape::Create {
                path: bundle_path(bundle),
                files: files
                    .iter()
                    .map(|put| (put.path.clone(), put.bytes.clone()))
                    .collect(),
            },
        ),
        Planned::BundleGone { bundle } => shaped(
            Lands::Content,
            Shape::Trash {
                path: bundle_path(bundle),
                files: files_under(bundle, ""),
            },
        ),
        Planned::Changed { bundle, changes } => {
            let shape = changed(bundle, changes, &files_under)?;
            match shape {
                Some(shape) => shaped(Lands::Content, shape),
                None => Ok(None),
            }
        }
    }
}

/// The step of changes to an existing bundle: one file written or taken
/// away, or several written into one new directory.
fn changed(
    bundle: &str,
    changes: &[Change],
    files_under: &impl Fn(&str, &str) -> Vec<String>,
) -> Result<Option<Shape>, LibraryTreeRejection> {
    let in_bundle = |path: &str| format!("{}/{path}", bundle_path(bundle));
    match changes {
        [] => Ok(None),
        [Change::Write(put)] => Ok(Some(Shape::Replace {
            path: in_bundle(&put.path),
            bytes: put.bytes.clone(),
        })),
        [Change::Remove { path }] => Ok(Some(Shape::Trash {
            path: in_bundle(path),
            files: Vec::new(),
        })),
        [Change::RemoveDir { path }] => Ok(Some(Shape::Trash {
            path: in_bundle(path),
            files: files_under(bundle, path),
        })),
        several => {
            let writes: Option<Vec<_>> = several
                .iter()
                .map(|change| match change {
                    Change::Write(put) => Some(put),
                    _ => None,
                })
                .collect();
            let common = writes.as_ref().and_then(|puts| {
                let dir = puts.first()?.path.rsplit_once('/')?.0.to_string();
                let under = format!("{dir}/");
                puts.iter()
                    .all(|put| put.path.starts_with(&under))
                    .then_some(dir)
            });
            match (writes, common) {
                (Some(puts), Some(dir)) if files_under(bundle, &dir).is_empty() => {
                    Ok(Some(Shape::Create {
                        path: in_bundle(&dir),
                        files: puts
                            .iter()
                            .map(|put| (put.path[dir.len() + 1..].to_string(), put.bytes.clone()))
                            .collect(),
                    }))
                }
                _ => Err(LibraryTreeRejection::OneChangePerCall {
                    paths: several.iter().map(change_path).collect(),
                }),
            }
        }
    }
}

fn change_path(change: &Change) -> String {
    match change {
        Change::Write(put) => put.path.clone(),
        Change::Remove { path } | Change::RemoveDir { path } => path.clone(),
    }
}

/// The files a planned write names, below its bundle (a layer's file, its
/// own name).
fn paths_of(planned: &Planned) -> Vec<String> {
    match planned {
        Planned::Layer { .. } => vec![CONFIG.to_string()],
        Planned::NewBundle { files, .. } => files.iter().map(|put| put.path.clone()).collect(),
        Planned::BundleGone { bundle } => vec![bundle.clone()],
        Planned::Changed { changes, .. } => changes.iter().map(change_path).collect(),
    }
}

/// A write's command: the step its shape is, against the token `seen` of
/// its path, the ids git would give what it writes asked of `observer` in
/// the tree `dir`, under `verb` and the evidence `evidence`.
pub fn intent_of(
    observer: &dyn TreeObserver,
    dir: &str,
    tree: &str,
    verb: &str,
    shape: &Shape,
    seen: u64,
    evidence: Evidence,
) -> Result<Intended, TreeError> {
    Ok(match shape {
        Shape::Replace { path, bytes } => Intended::Replace(ReplaceFile {
            name: tree.to_string(),
            verb: verb.to_string(),
            path: path.clone(),
            seen_sequence: seen,
            new_oid: observer.blob_id_of(dir, path, bytes)?,
            evidence,
        }),
        Shape::Create { path, files } => {
            let mut entries = Vec::new();
            for (rel, bytes) in files {
                entries.push(DirtyPath {
                    path: rel.clone(),
                    oid: observer.blob_id_of(dir, &format!("{path}/{rel}"), bytes)?,
                });
            }
            Intended::Create(CreateDirectory {
                name: tree.to_string(),
                verb: verb.to_string(),
                path: path.clone(),
                entries,
                evidence,
            })
        }
        Shape::Trash { path, .. } => Intended::Trash(TrashPath {
            name: tree.to_string(),
            verb: verb.to_string(),
            path: path.clone(),
            seen_sequence: seen,
            evidence,
        }),
    })
}

/// A write's command, as its step's kind.
#[derive(Clone, Debug, PartialEq)]
pub enum Intended {
    Replace(ReplaceFile),
    Create(CreateDirectory),
    Trash(TrashPath),
}

/// The paths a step changes, each with the id it leaves (empty: taken
/// away): what the re-observe looks at. A trash leaves gone each file of
/// its shape (the path itself, for a file).
pub fn expected(intended: &Intended, shape: &Shape) -> Vec<DirtyPath> {
    match intended {
        Intended::Replace(replace) => vec![DirtyPath {
            path: replace.path.clone(),
            oid: replace.new_oid.clone(),
        }],
        Intended::Create(create) => create
            .entries
            .iter()
            .map(|entry| DirtyPath {
                path: format!("{}/{}", create.path, entry.path),
                oid: entry.oid.clone(),
            })
            .collect(),
        Intended::Trash(trash) => {
            let files = match shape {
                Shape::Trash { files, .. } if !files.is_empty() => files
                    .iter()
                    .map(|file| format!("{}/{file}", trash.path))
                    .collect(),
                _ => vec![trash.path.clone()],
            };
            files
                .into_iter()
                .map(|path| DirtyPath {
                    path,
                    oid: String::new(),
                })
                .collect()
        }
    }
}

/// What a step left, as git sees it now: each path it changed with its id
/// there (empty where nothing is).
pub fn left_by(
    observer: &dyn TreeObserver,
    dir: &str,
    paths: &[DirtyPath],
) -> Result<Vec<DirtyPath>, TreeError> {
    let asked: Vec<&str> = paths.iter().map(|p| p.path.as_str()).collect();
    let ids: BTreeMap<String, String> = observer.blob_ids(dir, &asked)?;
    Ok(paths
        .iter()
        .map(|p| DirtyPath {
            path: p.path.clone(),
            oid: ids.get(&p.path).cloned().unwrap_or_default(),
        })
        .collect())
}

/// The cover of `tree`'s stream.
fn cover_of(tree: &library_tree::TreeName) -> angzarr_embedded::v1::Cover {
    angzarr_embedded::v1::Cover {
        domain: library_tree::LIBRARY_DOMAIN.to_string(),
        root: Some(tree.root()),
        edition: None,
    }
}

/// The book that sends `intended` to `tree`'s stream under the command id
/// `request_id` (a UUID's 16 bytes): the request's own id, so its one retry
/// is a DUPLICATE (AIP-155).
pub fn command_book(
    tree: &library_tree::TreeName,
    intended: &Intended,
    request_id: [u8; 16],
) -> angzarr_embedded::v1::CommandBook {
    use angzarr_embedded::v1::{command_page, Command, CommandPage, Uuid};
    let payload = match intended {
        Intended::Replace(command) => command.clone().to_wire_any(),
        Intended::Create(command) => command.clone().to_wire_any(),
        Intended::Trash(command) => command.clone().to_wire_any(),
    };
    angzarr_embedded::v1::CommandBook {
        cover: Some(cover_of(tree)),
        page: Some(CommandPage {
            body: Some(command_page::Body::Command(Command {
                payload: Some(payload),
                handling: None,
            })),
            command_id: Some(Uuid::from_bytes(request_id)),
            ..CommandPage::default()
        }),
    }
}

/// A command as the `Any` its book carries.
trait WireAny {
    fn to_wire_any(self) -> angzarr_embedded::Any;
}

impl<M: angzarr_embedded::markup::DomainMessage> WireAny for M {
    fn to_wire_any(self) -> angzarr_embedded::Any {
        angzarr_embedded::Any::from_msg(&self.to_wire()).expect("a library command encodes")
    }
}

/// The book that records a step's end on `tree`'s stream: `fact` keyed
/// `{kind}:{request_id}:{step}`, so its redelivery is a DUPLICATE.
pub fn step_book<M: angzarr_embedded::markup::DomainMessage>(
    tree: &library_tree::TreeName,
    fact: &M,
    key: &str,
) -> angzarr_embedded::v1::CommandBook {
    use angzarr_embedded::v1::{command_page, CommandPage, Fact, FactOrigin};
    angzarr_embedded::v1::CommandBook {
        cover: Some(cover_of(tree)),
        page: Some(CommandPage {
            body: Some(command_page::Body::Fact(Fact {
                payload: Some(
                    angzarr_embedded::Any::from_msg(&fact.to_wire())
                        .expect("a step's fact encodes"),
                ),
                origin: Some(FactOrigin {
                    external_id: key.to_string(),
                    description: "a library write's step".to_string(),
                }),
            })),
            ..CommandPage::default()
        }),
    }
}

#[cfg(test)]
#[path = "writes.test.rs"]
mod tests;
