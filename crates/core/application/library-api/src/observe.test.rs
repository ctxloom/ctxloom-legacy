//! The observation over the real library aggregate and its checkouts
//! projected inline, on an in-memory store: what a read records, and what
//! it refuses before anything is named or recorded.

use std::sync::{Arc, Mutex};

use angzarr_embedded::testing::{MemoryStore, Samples};
use angzarr_embedded::v1::{CommandBook, Cover, EventBook, EventPage};
use angzarr_embedded::{
    Answered, Failure, Handled, Kernel, OnRetryable, Registry, Rejected, Sampled, StoreError,
    StreamRef,
};
use ctxloom_ports::{IgnoreRule, Motion, PathStatus};
use library_checkout::LibraryCheckouts;
use library_tree::{LibraryTreeRejection, TreeName, LIBRARY_DOMAIN};
use run_ports::{Edge, Unserved};
use sessions::ProjectName;

use super::{CheckoutHarps, Observations, ReadError, Site};
use crate::fakes::Git;
use crate::LibraryApiNotice;

const BUNDLE: &str = "content/bundles/v2/demo/bundle.yaml";
const HARP: &str = "brisk-copper-moth";

/// The kernel with the library aggregate and its checkouts, over memory.
struct World {
    registry: Registry,
    store: MemoryStore,
    samples: Mutex<Samples>,
    /// How many books it was handed.
    handled: Mutex<u32>,
}

impl World {
    fn new(checkouts: &LibraryCheckouts) -> Arc<World> {
        let mut registry = Registry::new();
        registry
            .aggregate::<library_tree::LibraryAggregate>(library_tree::LibraryAggregate::default)
            .unwrap();
        registry
            .projector_handler(&library_checkout::SOURCES, checkouts.clone())
            .unwrap();
        Arc::new(World {
            registry,
            store: MemoryStore::default(),
            samples: Mutex::new(Samples::starting_at(1_000)),
            handled: Mutex::new(0),
        })
    }
}

impl Edge for World {
    async fn serve(&self, book: CommandBook, _: run_ports::Arrival) -> Result<Answered, Unserved> {
        self.handle(book).await.map_err(Unserved::Failed)
    }

    async fn handle(&self, book: CommandBook) -> Result<Answered, Failure<Rejected>> {
        *self.handled.lock().unwrap() += 1;
        let sampled: Sampled = self.samples.lock().unwrap().next_sample();
        let kernel = Kernel::new(&self.registry).on_retryable(OnRetryable {
            max_retries: 0,
            ..OnRetryable::default()
        });
        let cover = book.cover.clone();
        let handled: Handled = kernel.handle_any(&self.store, book, &sampled).await?;
        let events = handled
            .books()
            .into_iter()
            .find(|book| book.cover == cover)
            .unwrap_or(EventBook {
                cover,
                ..EventBook::default()
            });
        Ok(Answered {
            events: Some(events),
            delivery: handled.delivery(),
        })
    }

    async fn events(&self, cover: &Cover, from: u64) -> Result<Vec<EventPage>, StoreError> {
        let stream = StreamRef::try_from(cover)?;
        let slice =
            angzarr_embedded::angzarr_ports::EventStore::read(&self.store, stream, from, None)
                .await?;
        Ok(slice.pages)
    }
}

/// A harp minted once, counted.
#[derive(Default)]
struct Harps(Mutex<u32>);

impl CheckoutHarps for Harps {
    fn harp(&self, _tree: &str) -> Result<String, ctxloom_ports::Failure> {
        *self.0.lock().unwrap() += 1;
        Ok(HARP.to_string())
    }
}

fn site() -> Site {
    Site::Checkout {
        project: ProjectName::new("calm-otter").unwrap(),
        root: "/work/demo".to_string(),
        tree: "/work/demo/.ctxloom".to_string(),
    }
}

fn tree() -> TreeName {
    TreeName::checkout(ProjectName::new("calm-otter").unwrap(), HARP).unwrap()
}

struct Under {
    world: Arc<World>,
    checkouts: LibraryCheckouts,
    harps: Arc<Harps>,
    observations: Observations<World>,
}

fn under() -> Under {
    let checkouts = LibraryCheckouts::default();
    let world = World::new(&checkouts);
    let harps = Arc::new(Harps::default());
    let observations = Observations::new(
        Arc::clone(&world),
        checkouts.clone(),
        Arc::clone(&harps) as Arc<dyn CheckoutHarps>,
    );
    Under {
        world,
        checkouts,
        harps,
        observations,
    }
}

/// The tree's events, as its stream holds them.
async fn events(world: &World) -> Vec<EventPage> {
    let cover = Cover {
        domain: LIBRARY_DOMAIN.to_string(),
        root: Some(tree().root()),
        edition: None,
    };
    world.events(&cover, 0).await.unwrap()
}

#[tokio::test(flavor = "current_thread")]
async fn a_first_read_records_the_baseline_and_a_read_that_sees_nothing_new_records_nothing() {
    let under = under();
    let git = Git::at("c1", "t1");
    let observed = under.observations.observe(&git, &site()).await.unwrap();
    assert_eq!(observed.tree, tree());
    assert!(observed.notices.is_empty());
    under.observations.observe(&git, &site()).await.unwrap();
    assert_eq!(events(&under.world).await.len(), 1);
    assert_eq!(*under.world.handled.lock().unwrap(), 1);
    let held = under
        .checkouts
        .checkouts()
        .tree(&tree().root())
        .cloned()
        .unwrap();
    assert!(held.baselined);
    assert_eq!(held.head, "c1");
}

#[tokio::test(flavor = "current_thread")]
async fn a_change_is_recorded_once_by_the_read_that_sees_it() {
    let under = under();
    under
        .observations
        .observe(&Git::at("c1", "t1"), &site())
        .await
        .unwrap();
    let edited = Git::at("c1", "t1")
        .file(BUNDLE, "b2")
        .says(BUNDLE, PathStatus::Modified);
    under.observations.observe(&edited, &site()).await.unwrap();
    under.observations.observe(&edited, &site()).await.unwrap();
    assert_eq!(events(&under.world).await.len(), 2);
    let held = under
        .checkouts
        .checkouts()
        .tree(&tree().root())
        .cloned()
        .unwrap();
    assert_eq!(held.dirty.get(BUNDLE).map(String::as_str), Some("b2"));
    assert_eq!(held.token(BUNDLE), 1);
}

#[tokio::test(flavor = "current_thread")]
async fn two_reads_that_see_one_change_against_one_state_record_it_once() {
    // Forced: both sights are made against the state before either is
    // recorded, as two reads at once make them.
    let under = under();
    under
        .observations
        .observe(&Git::at("c1", "t1"), &site())
        .await
        .unwrap();
    let edited = Git::at("c1", "t1")
        .file(BUNDLE, "b2")
        .says(BUNDLE, PathStatus::Modified);
    let held = under
        .checkouts
        .checkouts()
        .tree(&tree().root())
        .cloned()
        .unwrap();
    let first = crate::sight(&edited, "/work/demo/.ctxloom", &tree(), &held).unwrap();
    let second = crate::sight(&edited, "/work/demo/.ctxloom", &tree(), &held).unwrap();
    for sight in [first, second] {
        let fact = sight.fact.unwrap();
        under
            .world
            .handle(library_tree::observation_book(&tree(), &fact))
            .await
            .unwrap();
    }
    assert_eq!(events(&under.world).await.len(), 2);
}

#[tokio::test(flavor = "current_thread")]
async fn a_read_outside_git_is_refused_before_its_checkout_is_named() {
    let under = under();
    let git = Git::default();
    let refused = under.observations.observe(&git, &site()).await.unwrap_err();
    assert!(matches!(
        refused,
        ReadError::Refused(LibraryTreeRejection::NotInGit { ref root }) if root == "/work/demo"
    ));
    assert_eq!(*under.harps.0.lock().unwrap(), 0);
    assert_eq!(*under.world.handled.lock().unwrap(), 0);
}

#[tokio::test(flavor = "current_thread")]
async fn a_read_of_a_library_git_ignores_is_refused_before_its_checkout_is_named() {
    let under = under();
    let mut git = Git::at("c1", "t1");
    git.ignored = Some(IgnoreRule {
        source: ".gitignore".to_string(),
        line: 1,
        pattern: ".ctxloom/".to_string(),
    });
    let refused = under.observations.observe(&git, &site()).await.unwrap_err();
    assert!(matches!(
        refused,
        ReadError::Refused(LibraryTreeRejection::LibraryIgnored { .. })
    ));
    assert_eq!(*under.harps.0.lock().unwrap(), 0);
}

#[tokio::test(flavor = "current_thread")]
async fn the_home_is_observed_without_the_project_checks() {
    let under = under();
    let git = Git {
        work_tree: None,
        ..Git::at("c1", "t1")
    };
    let home = Site::Home {
        tree: "/h/.ctxloom".to_string(),
    };
    let observed = under.observations.observe(&git, &home).await.unwrap();
    assert_eq!(observed.tree, TreeName::home(HARP).unwrap());
    assert!(!git.calls().iter().any(|c| c == "work_tree"));
    assert_eq!(home.tree(), "/h/.ctxloom");
}

#[tokio::test(flavor = "current_thread")]
async fn a_read_during_a_git_operation_says_so_naming_the_tree() {
    let under = under();
    let mut git = Git::at("c1", "t1");
    git.motion = Some(Motion::Rebasing);
    let observed = under.observations.observe(&git, &site()).await.unwrap();
    assert_eq!(
        observed.notices,
        vec![LibraryApiNotice::TreeInMotion {
            motion: Motion::Rebasing,
            root: "/work/demo/.ctxloom".to_string(),
        }]
    );
}

#[tokio::test(flavor = "current_thread")]
async fn git_failing_fails_the_read() {
    struct Broken;
    impl ctxloom_ports::TreeObserver for Broken {
        fn work_tree(&self, _: &str) -> Result<Option<String>, ctxloom_ports::TreeError> {
            Ok(Some("/work/demo".to_string()))
        }
        fn check_ignore(
            &self,
            _: &str,
            _: &str,
        ) -> Result<Option<IgnoreRule>, ctxloom_ports::TreeError> {
            Ok(None)
        }
        fn init_repository(
            &self,
            _: &str,
            _: Option<&str>,
        ) -> Result<(), ctxloom_ports::TreeError> {
            Ok(())
        }
        fn head(&self, _: &str) -> Result<Option<String>, ctxloom_ports::TreeError> {
            Ok(None)
        }
        fn library_tree(
            &self,
            _: &str,
            _: &str,
        ) -> Result<Option<String>, ctxloom_ports::TreeError> {
            Ok(None)
        }
        fn status(
            &self,
            _: &str,
            _: &[&str],
        ) -> Result<Vec<ctxloom_ports::StatusEntry>, ctxloom_ports::TreeError> {
            Ok(Vec::new())
        }
        fn diff_tree(
            &self,
            _: &str,
            _: &str,
            _: &str,
            _: &[&str],
        ) -> Result<Vec<ctxloom_ports::TreeChange>, ctxloom_ports::TreeError> {
            Ok(Vec::new())
        }
        fn blob_ids(
            &self,
            _: &str,
            _: &[&str],
        ) -> Result<std::collections::BTreeMap<String, String>, ctxloom_ports::TreeError> {
            Ok(Default::default())
        }
        fn blob_id_of(
            &self,
            _: &str,
            _: &str,
            _: &[u8],
        ) -> Result<String, ctxloom_ports::TreeError> {
            unreachable!("not asked")
        }

        fn in_motion(&self, _: &str) -> Result<Option<Motion>, ctxloom_ports::TreeError> {
            Err(ctxloom_ports::TreeError::GitMissing(Box::new(
                LibraryTreeRejection::NotInGit {
                    root: "stand-in".to_string(),
                },
            )))
        }
    }
    let under = under();
    let failed = under
        .observations
        .observe(&Broken, &site())
        .await
        .unwrap_err();
    assert!(matches!(failed, ReadError::Failed(_)));
    assert_eq!(*under.world.handled.lock().unwrap(), 0);
}

/// git as the caller runs it: a fresh table each read, the environment it
/// was asked with kept.
struct Callers {
    git: Box<dyn Fn() -> Git + Send + Sync>,
    home: Option<String>,
    env: Mutex<Vec<Vec<(String, String)>>>,
}

impl super::Observers for Callers {
    fn observer(&self, env: &[(String, String)]) -> Box<dyn ctxloom_ports::TreeObserver> {
        self.env.lock().unwrap().push(env.to_vec());
        Box::new((self.git)())
    }

    fn home_tree(&self) -> Option<String> {
        self.home.clone()
    }
}

fn callers(git: impl Fn() -> Git + Send + Sync + 'static, home: Option<&str>) -> Callers {
    Callers {
        git: Box::new(git),
        home: home.map(str::to_string),
        env: Mutex::new(Vec::new()),
    }
}

fn home_events_root() -> angzarr_embedded::v1::Uuid {
    TreeName::home(HARP).unwrap().root()
}

#[tokio::test(flavor = "current_thread")]
async fn a_read_observes_its_checkout_then_the_home_in_the_callers_environment() {
    let under = under();
    let callers = callers(|| Git::at("c1", "t1"), Some("/h/.ctxloom"));
    let env = vec![("PATH".to_string(), "/usr/bin".to_string())];
    let project = ProjectName::new("calm-otter").unwrap();
    let notices = under
        .observations
        .observe_read(&callers, &env, Some((project, "/work/demo")))
        .await
        .unwrap();
    assert!(notices.is_empty());
    assert_eq!(*callers.env.lock().unwrap(), vec![env]);
    assert_eq!(events(&under.world).await.len(), 1);
    let home = Cover {
        domain: LIBRARY_DOMAIN.to_string(),
        root: Some(home_events_root()),
        edition: None,
    };
    assert_eq!(under.world.events(&home, 0).await.unwrap().len(), 1);
}

// G2, the home step in the daemon (step 10a U2): a home whose library is in
// no work tree, or in one that ignores it, is made a repository of its own
// at its first read, and the read says so; one already a repository is
// used as it is, and nothing is said.
#[tokio::test(flavor = "current_thread")]
async fn a_home_in_no_work_tree_or_one_git_ignores_is_made_a_repository_and_said() {
    for git in [
        Box::new(|| Git {
            work_tree: None,
            ..Git::at("c1", "t1")
        }) as Box<dyn Fn() -> Git + Send + Sync>,
        Box::new(|| {
            let mut git = Git::at("c1", "t1");
            git.work_tree = Some("/h".to_string());
            git.ignored = Some(IgnoreRule {
                source: "/h/.gitignore".to_string(),
                line: 1,
                pattern: ".ctxloom".to_string(),
            });
            git
        }),
    ] {
        let under = under();
        let callers = Callers {
            git,
            home: Some("/h/.ctxloom".to_string()),
            env: Mutex::new(Vec::new()),
        };
        let said = under
            .observations
            .observe_read(&callers, &[], None)
            .await
            .unwrap();
        assert_eq!(
            said,
            vec![LibraryApiNotice::HomeRepositoryMade {
                path: "/h/.ctxloom".to_string()
            }]
        );
    }
}

#[tokio::test(flavor = "current_thread")]
async fn a_home_that_is_a_repository_already_is_used_as_it_is_and_nothing_said() {
    let under = under();
    let callers = callers(
        || Git {
            work_tree: Some("/h/.ctxloom".to_string()),
            ..Git::at("c1", "t1")
        },
        Some("/h/.ctxloom"),
    );
    let said = under
        .observations
        .observe_read(&callers, &[], None)
        .await
        .unwrap();
    assert!(said.is_empty(), "{said:?}");
}

#[tokio::test(flavor = "current_thread")]
async fn a_refused_checkout_observes_no_home_and_says_why() {
    let under = under();
    let callers = callers(Git::default, Some("/h/.ctxloom"));
    let project = ProjectName::new("calm-otter").unwrap();
    let refused = under
        .observations
        .observe_read(&callers, &[], Some((project, "/work/demo")))
        .await
        .unwrap_err();
    assert!(matches!(
        refused,
        ReadError::Refused(LibraryTreeRejection::NotInGit { .. })
    ));
    assert_eq!(*under.world.handled.lock().unwrap(), 0);
}

#[tokio::test(flavor = "current_thread")]
async fn both_trees_say_their_motion() {
    let under = under();
    let callers = callers(
        || {
            let mut git = Git::at("c1", "t1");
            git.motion = Some(Motion::Merging);
            git
        },
        Some("/h/.ctxloom"),
    );
    let project = ProjectName::new("calm-otter").unwrap();
    let notices = under
        .observations
        .observe_read(&callers, &[], Some((project, "/work/demo/")))
        .await
        .unwrap();
    let roots: Vec<String> = notices
        .into_iter()
        .map(|notice| match notice {
            LibraryApiNotice::TreeInMotion { root, .. } => root,
            other => panic!("{other:?}"),
        })
        .collect();
    assert_eq!(roots, vec!["/work/demo/.ctxloom", "/h/.ctxloom"]);
}

#[test]
fn a_checkouts_library_is_its_ctxloom() {
    let project = ProjectName::new("calm-otter").unwrap();
    assert_eq!(
        Site::checkout(project.clone(), "/work/demo"),
        Site::Checkout {
            project,
            root: "/work/demo".to_string(),
            tree: "/work/demo/.ctxloom".to_string(),
        }
    );
}

// A home git does not hold (no work tree, after the home step) is not
// observed: nothing is recorded for it.
#[tokio::test(flavor = "current_thread")]
async fn a_home_git_does_not_hold_is_not_observed() {
    let under = under();
    let callers = callers(
        || Git {
            work_tree: None,
            ..Git::at("c1", "t1")
        },
        Some("/h/.ctxloom"),
    );
    under
        .observations
        .observe_read(&callers, &[], None)
        .await
        .unwrap();
    let home = Cover {
        domain: LIBRARY_DOMAIN.to_string(),
        root: Some(home_events_root()),
        edition: None,
    };
    assert!(under.world.events(&home, 0).await.unwrap().is_empty());
}

/// The operations port, counting the sweeps of each tree.
#[derive(Default)]
struct Sweeps(Mutex<Vec<String>>);

impl ctxloom_ports::Operations for Sweeps {
    fn operate(
        &self,
        _: &str,
        _: &str,
        _: &ctxloom_ports::Operation,
        _: &dyn Fn(ctxloom_ports::Point),
    ) -> Result<ctxloom_ports::Operated, ctxloom_ports::Failure> {
        unreachable!("an observation operates nothing")
    }

    fn sweep_staging(&self, tree: &str) -> Result<(), ctxloom_ports::Failure> {
        self.0.lock().unwrap().push(tree.to_string());
        Ok(())
    }

    fn purge_slot(&self, _: &str, _: &str) -> Result<(), ctxloom_ports::Failure> {
        unreachable!("an observation purges nothing")
    }
}

// S50: what a killed write left staged is swept when a daemon starts, as
// the daemon first observes the tree (a read or a write), and never again
// in that daemon's life, so a write it stages later is never swept from
// under it. The trash is not swept (the port's own test).
#[tokio::test(flavor = "current_thread")]
async fn a_tree_s_staging_is_swept_at_its_first_observation_alone() {
    let checkouts = LibraryCheckouts::default();
    let world = World::new(&checkouts);
    let sweeps = Arc::new(Sweeps::default());
    let observations = Observations::new(
        Arc::clone(&world),
        checkouts,
        Arc::new(Harps::default()) as Arc<dyn CheckoutHarps>,
    )
    .sweeping(Arc::clone(&sweeps) as Arc<dyn ctxloom_ports::Operations>);
    let git = Git::at("c1", "t1");
    observations.observe(&git, &site()).await.unwrap();
    observations.observe(&git, &site()).await.unwrap();
    let home = Site::Home {
        tree: "/home/alice/.ctxloom".to_string(),
    };
    observations.observe(&git, &home).await.unwrap();
    assert_eq!(
        *sweeps.0.lock().unwrap(),
        vec!["/work/demo/.ctxloom", "/home/alice/.ctxloom"]
    );
}

// A tree a read is refused (outside git) is not touched.
#[tokio::test(flavor = "current_thread")]
async fn a_refused_tree_is_not_swept() {
    let checkouts = LibraryCheckouts::default();
    let world = World::new(&checkouts);
    let sweeps = Arc::new(Sweeps::default());
    let observations = Observations::new(
        world,
        checkouts,
        Arc::new(Harps::default()) as Arc<dyn CheckoutHarps>,
    )
    .sweeping(Arc::clone(&sweeps) as Arc<dyn ctxloom_ports::Operations>);
    let refused = observations.observe(&Git::default(), &site()).await;
    assert!(refused.is_err());
    assert!(sweeps.0.lock().unwrap().is_empty());
}
