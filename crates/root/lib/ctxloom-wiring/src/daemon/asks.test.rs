use std::sync::mpsc;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use daemon_proto::v1;
use run_ports::{Asker, PermissionAnswer, PermissionAsk, PermissionAsks, PermissionRouter};

use super::{Boxed, RoutedAsks, Routes};

/// How long a test waits to be told what it forced.
const TOLD: Duration = Duration::from_secs(20);

/// The daemon's routed calls, scripted: a create that waits until the test
/// lets it go (when `gate` is given), then the answer `answer`; each call
/// told the test.
struct Scripted {
    gate: Option<Mutex<mpsc::Receiver<()>>>,
    entered: Mutex<mpsc::Sender<()>>,
    fails: bool,
    answer: PermissionAnswer,
    withdrawn: Mutex<Vec<(String, String)>>,
    answered: Mutex<usize>,
    /// When set, the answer waits until a withdrawal is made, then is
    /// cancelled (as a withdrawn ask's wait ends).
    waits: Option<Withdrawals>,
}

/// How a waiting answer is told of a withdrawal: its sender, its receiver.
type Withdrawals = (Mutex<mpsc::Sender<()>>, Mutex<mpsc::Receiver<()>>);

fn approval() -> v1::Approval {
    v1::Approval {
        name: "projects/p/approvals/ask-tidy-wren".to_string(),
        ..v1::Approval::default()
    }
}

impl Routes for Scripted {
    fn create<'a>(
        &'a self,
        _: &'a Asker,
        _: &'a PermissionAsk,
    ) -> Boxed<'a, Result<v1::Approval, String>> {
        Box::pin(async move {
            let _ = self.entered.lock().unwrap().send(());
            if let Some(gate) = &self.gate {
                gate.lock().unwrap().recv_timeout(TOLD).expect("let go");
            }
            match self.fails {
                true => Err("the store is gone".to_string()),
                false => Ok(approval()),
            }
        })
    }

    fn answer<'a>(&'a self, _: &'a v1::Approval) -> Boxed<'a, Option<PermissionAnswer>> {
        Box::pin(async move {
            *self.answered.lock().unwrap() += 1;
            if let Some((_, withdrawn)) = &self.waits {
                let _ = self.entered.lock().unwrap().send(());
                withdrawn
                    .lock()
                    .unwrap()
                    .recv_timeout(TOLD)
                    .expect("withdrawn");
                return Some(PermissionAnswer::Cancelled);
            }
            Some(self.answer.clone())
        })
    }

    fn withdraw<'a>(&'a self, approval: &'a v1::Approval, reason: &'a str) -> Boxed<'a, ()> {
        Box::pin(async move {
            self.withdrawn
                .lock()
                .unwrap()
                .push((approval.name.clone(), reason.to_string()));
            if let Some((tell, _)) = &self.waits {
                let _ = tell.lock().unwrap().send(());
            }
        })
    }
}

struct Rig {
    runtime: tokio::runtime::Runtime,
    routes: Arc<Scripted>,
    entered: mpsc::Receiver<()>,
    open: Option<mpsc::Sender<()>>,
    said: Arc<Mutex<Vec<String>>>,
}

fn rig(gated: bool, fails: bool, answer: PermissionAnswer) -> Rig {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()
        .unwrap();
    let (entered_tx, entered) = mpsc::channel();
    let (open, gate) = mpsc::channel();
    let routes = Arc::new(Scripted {
        gate: gated.then(|| Mutex::new(gate)),
        entered: Mutex::new(entered_tx),
        fails,
        answer,
        withdrawn: Mutex::new(Vec::new()),
        answered: Mutex::new(0),
        waits: None,
    });
    Rig {
        runtime,
        routes,
        entered,
        open: gated.then_some(open),
        said: Arc::default(),
    }
}

impl Rig {
    fn asks(&self, given: bool) -> Arc<dyn PermissionAsks> {
        let late: super::LateRoutes = Arc::new(OnceLock::new());
        if given {
            let routes: Arc<dyn Routes> = self.routes.clone();
            let _ = late.set(Arc::downgrade(&routes));
        }
        let said = Arc::clone(&self.said);
        RoutedAsks {
            routes: late,
            runtime: self.runtime.handle().clone(),
            say: Arc::new(move |line: &str| said.lock().unwrap().push(line.to_string())),
        }
        .asks(&Asker {
            session: "S".to_string(),
            approval_timeout: Duration::from_secs(300),
            ..Asker::default()
        })
    }
}

fn ask() -> PermissionAsk {
    PermissionAsk {
        request_id: "t1".to_string(),
        tool_name: "bash".to_string(),
        input_json: "{}".to_string(),
        options: Vec::new(),
    }
}

/// `asks` asked on a thread of its own, as an engine's adapter asks it.
fn asked_aside(asks: &Arc<dyn PermissionAsks>) -> std::thread::JoinHandle<PermissionAnswer> {
    let asks = Arc::clone(asks);
    std::thread::spawn(move || asks.ask(&ask()))
}

#[test]
fn an_ask_is_recorded_waited_on_and_answered_with_the_option_chosen() {
    let rig = rig(false, false, PermissionAnswer::Selected("once".to_string()));
    let asks = rig.asks(true);
    assert_eq!(
        asked_aside(&asks).join().unwrap(),
        PermissionAnswer::Selected("once".to_string())
    );
    assert_eq!(*rig.routes.answered.lock().unwrap(), 1);
    // Answered: a withdrawal after finds nothing to withdraw.
    asks.withdraw("t1", "steered");
    assert!(rig.routes.withdrawn.lock().unwrap().is_empty());
}

// The race, forced: the withdrawal comes while the ask is being recorded.
// It is held, and made once the ask is recorded; the ask is not waited on.
#[test]
fn a_withdrawal_while_the_ask_is_recorded_is_made_once_it_is() {
    let mut rig = rig(true, false, PermissionAnswer::Selected("once".to_string()));
    let asks = rig.asks(true);
    let asking = asked_aside(&asks);
    rig.entered.recv_timeout(TOLD).expect("the create began");
    asks.withdraw("t1", "steered");
    assert!(rig.routes.withdrawn.lock().unwrap().is_empty(), "held");
    rig.open.take().unwrap().send(()).unwrap();
    assert_eq!(asking.join().unwrap(), PermissionAnswer::Cancelled);
    assert_eq!(
        *rig.routes.withdrawn.lock().unwrap(),
        [(approval().name, "steered".to_string())]
    );
    assert_eq!(*rig.routes.answered.lock().unwrap(), 0);
}

#[test]
fn a_withdrawal_of_an_ask_waited_on_is_made_at_once_and_ends_its_wait() {
    let mut rig = rig(false, false, PermissionAnswer::Selected("once".to_string()));
    let (tell, withdrawn) = mpsc::channel();
    Arc::get_mut(&mut rig.routes).unwrap().waits = Some((Mutex::new(tell), Mutex::new(withdrawn)));
    let asks = rig.asks(true);
    let asking = asked_aside(&asks);
    rig.entered.recv_timeout(TOLD).expect("the create began");
    // Forced: the ask is recorded, and its answer waited on.
    rig.entered.recv_timeout(TOLD).expect("the wait began");
    asks.withdraw("t1", "interrupted");
    assert_eq!(asking.join().unwrap(), PermissionAnswer::Cancelled);
    assert_eq!(
        *rig.routes.withdrawn.lock().unwrap(),
        [(approval().name, "interrupted".to_string())]
    );
}

#[test]
fn an_ask_that_cannot_be_recorded_or_has_no_daemon_is_answered_cancelled() {
    let rig = rig(false, true, PermissionAnswer::Selected("once".to_string()));
    assert_eq!(
        asked_aside(&rig.asks(true)).join().unwrap(),
        PermissionAnswer::Cancelled
    );
    assert!(rig.said.lock().unwrap()[0].contains("the store is gone"));
    assert_eq!(
        asked_aside(&rig.asks(false)).join().unwrap(),
        PermissionAnswer::Cancelled
    );
    assert_eq!(*rig.routes.answered.lock().unwrap(), 0);
}
