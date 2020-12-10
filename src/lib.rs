use std::fmt::Debug;
use std::hash::Hash;
use std::future::Future;
use std::sync::Mutex;
use std::collections::VecDeque;
use std::collections::HashMap;
use std::pin::Pin;
use std::{any::Any, time::Duration};
use delegate::delegate;
use std::collections::hash_map::Drain;

mod progress_compute;
pub use progress_compute::*;

mod progress_vars;
pub use progress_vars::*;

// pub type ProgressVars = HashMap<String, Box<dyn Any + Send>>;
pub type TaskResult = Result<Option<ProgressVars>, String>;
type PinBoxFuture = Pin<Box<dyn Future<Output = TaskResult> + Send>>;
type PinBoxFutureSimple = Pin<Box<dyn Future<Output = ()> + Send>>;

/// a stage holds a single future. it is an Option<>, but
/// a ProgressItem will not work without it being set. its only optional
/// to make it convenient to create a stage via
/// ```rs
/// Stage::new("stage-name").set_task_from_future(my_async_function())
/// ```
pub struct Stage {
    pub name: Option<Box<dyn Debug + Send>>,
    task: Option<PinBoxFuture>,
}


/// same as `make_stage_simple`, but the $func must return a TaskResult
#[macro_export]
macro_rules! make_stage {
    ($func:ident; $($t:tt)*) => (
        Stage::make(
            stringify!($func),
            $func( $($t)* )
        )
    )
}

/// a convenience macro to turn a function call into a stage.
/// Here's what making a stage would look like without this macro:
/// ```rs
/// pub async fn my_task(key: String) {}
///
/// pub fn uses_stage(key: String) {
///   let my_stage = Stage::make_simple("my_task", my_task(key));
/// }
/// ```
/// And here's how this macro makes stage creation simpler:
/// ```rs
/// pub fn uses_stage(key: String) {
///   let my_stage = make_stage_simple!(my_task; key);
/// }
/// ```
#[macro_export]
macro_rules! make_stage_simple {
    ($func:ident; $($t:tt)*) => (
        Stage::make_simple(
            stringify!($func),
            $func( $($t)* )
        )
    )
}

impl Stage {
    pub fn new(name: impl Debug + Send + 'static) -> Self {
        Stage {
            name: Some(Box::new(name)),
            task: None,
        }
    }

    pub fn set_task_from_future<F: Future<Output = TaskResult> + Send + 'static>(
        mut self,
        future: F
    ) -> Self {
        self.task = Some(Box::pin(future));
        self
    }

    pub fn set_task_from_simple_future<F: Future<Output = ()> + Send + 'static>(
        mut self,
        future: F
    ) -> Self {
        self.task = Some(Box::pin(async move {
            future.await;
            Ok(None)
        }));
        self
    }

    /// the most convenient way to make a stage. pass in a name (anything
    /// that can be debugged and send) and the actual future. This future must
    /// return a TaskResult, but if your future doesnt return a TaskResult,
    /// you can use make_simple instead.
    /// # Example:
    /// ```rs
    /// #[derive(Debug)]
    /// pub enum MyStages {
    ///     Stage1,
    ///     Stage2,
    /// }
    /// pub async fn some_async_function(thing: &'static str) -> TaskResult {
    ///   // something asynchronous...
    /// }
    /// Stage::make(MyStages::Stage1, some_async_function("something"))
    /// ```
    pub fn make<F: Future<Output = TaskResult> + Send + 'static>(
        name: impl Debug + Send + 'static,
        future: F,
    ) -> Self {
        Stage::new(name).set_task_from_future(future)
    }

    /// like make(), but the future you pass in has to return nothing instead
    /// of a TaskResult
    pub fn make_simple<F: Future<Output = ()> + Send + 'static>(
        name: impl Debug + Send + 'static,
        future: F,
    ) -> Self {
        Stage::new(name).set_task_from_future(async move {
            future.await;
            Ok(None)
        })
    }
}

#[derive(Debug, Clone)]
pub struct ProgressError {
    pub name: Option<String>,
    pub progress_index: usize,
    pub error_string: String,
}

/// meant to be used as a clone of a ProgressItem for when
/// you want to view the state of a progress item without
/// manually getting all of the fields, and unwrapping
/// the internal structure. This StageView
/// represents the view of a single stage. but from a ProgressItem,
/// you should be able to get a vec of stage views of all of the stages
/// that its already done, the current stage it is on, and  the remaining
/// stages it needs to do
#[derive(Debug)]
pub struct StageView {
    pub progress_percent: f64,
    pub name: String,
    pub index: usize,
    pub errored: Option<ProgressError>,
    pub currently_processing: bool,
}

impl From<&mut ProgressItem> for Vec<StageView> {
    fn from(orig: &mut ProgressItem) -> Self {
        // do all the past stages first
        let mut stages = vec![];
        let mut stage_index = 0;
        for past_stage_name in &orig.past_stage_names {
            stages.push(StageView {
                progress_percent: 100.0, // if its a past stage, 100% is implied
                name: past_stage_name.clone(),
                index: stage_index,
                errored: None, // if its a past stage it is implied that it is not errored
                currently_processing: false, // also implied because it is done
            });
            stage_index += 1;
        }

        // do the current stage
        let current_stage_name = orig.get_stage_name();
        stages.push(StageView {
            name: current_stage_name,
            progress_percent: orig.get_progress_percent(),
            index: stage_index,
            errored: orig.errored.clone(),
            currently_processing: orig.processing_stage,
        });
        stage_index += 1;

        if orig.stages.len() == 0 && !orig.processing_stage {
            // this means that the "current"
            // stage above is actually done, so we can treat it
            // as done:
            let last_stage_index = stages.len() - 1;
            stages[last_stage_index].progress_percent = 100.0;
        } else {
            // do the remaining stages
            for stage in orig.stages.iter() {
                stages.push(StageView {
                    name: match stage.name {
                        Some(ref name) => format!("{:?}", name),
                        None => stage_index.to_string(),
                    },
                    progress_percent: 0.0, // implied because it hasnt started yet,
                    index: stage_index,
                    errored: None, // implied because it hasnt started yet
                    currently_processing: false,
                });
                stage_index += 1;
            }
        }

        stages
    }
}

/// This struct is created by the user, but none of the struct members
/// need to be managed by the user. As the user you just need to create stages
/// to register with this progress item, and call some setters like set_max_lock_attempts()
/// as needed. Once this ProgressItem is created, and your stages are registered,
/// you should create a key, then
/// you can call progressitem.start() and pass in that key,
/// and then insert this progress item into the ProgressHolder with that
/// key. The ProgressItem will internally use tokio and keep passing a reference of its holder
/// and the key around such that every time the current stage ends, it can asynchronously spawn
/// the next stage. If there is an error handling the stage, the ProgressItem will be 'errored'
/// which means no future progress will be made, and a field will be set to view the error message
pub struct ProgressItem {
    past_stage_names: Vec<String>,
    stages: VecDeque<Stage>,
    started: bool,
    numstages: usize,
    current_stage: Option<(usize, Stage)>,
    progress: ProgressCompute,
    errored: Option<ProgressError>,
    done: bool,
    max_lock_attempts: usize,
    lock_attempt_wait: u64,
    paused: bool,
    pause_resume: VecDeque<PinBoxFutureSimple>,
    processing_stage: bool,
    vars: ProgressVars,
}

impl Default for ProgressItem {
    fn default() -> Self {
        ProgressItem {
            past_stage_names: vec![],
            numstages: 0,
            stages: VecDeque::new(),
            started: false,
            current_stage: None,
            errored: None,
            done: false,
            progress: ProgressCompute::default(),
            max_lock_attempts: 3,
            lock_attempt_wait: 1000,
            paused: false,
            pause_resume: VecDeque::new(),
            processing_stage: false,
            vars: ProgressVars::default(),
        }
    }
}

impl ProgressItem {
    delegate_progressvars_on!(vars);
    delegate_progresscompute_on!(progress);

    // TODO: decide what to do with name... should it be part of progress item or not?
    pub fn new() -> Self {
        Self::default()
    }

    /// the default max_lock_attempts is 3. you can provide an alternate max_lock_attempts
    /// setting 0 means infinite, not 0 attempts. if it was 0 attempts, nothing would get done
    pub fn set_max_lock_attempts(mut self, max: usize) -> Self {
        self.max_lock_attempts = max;
        self
    }

    /// duration in milliseconds. by default, we wait 1000 milliseconds
    /// before consecutive lock attempts, but you can customize this.
    pub fn set_lock_attempt_duration(mut self, duration: u64) -> Self {
        self.lock_attempt_wait = duration;
        self
    }

    pub fn get_progress_error(&self) -> &Option<ProgressError> {
        &self.errored
    }

    pub fn has_started(&self) -> bool { self.started }

    pub fn is_done(&self) -> bool { self.done }

    /// returns a tuple where 0 is the current stage number
    /// and 1 is the max number of stages. note: these are not indicies.
    /// so if your progress item has one stage, then this will return (1, 1)
    /// so that means it will return (1, 1) while it is doing the first(and only) stage
    /// and also when it is done with that stage, it will still return (1, 1). If you
    /// want to know if this progress item is done or not, use is_done() instead.
    /// if there is an error, or if the progress hasnt started yet, returns (0, 0)
    pub fn get_stage_progress(&self) -> (usize, usize) {
        if !self.started { return (0, 0); }
        match self.current_stage {
            None => (0, 0),
            Some(ref tuple) => (tuple.0, self.numstages),
        }
    }

    pub fn get_stage_name(&self) -> String {
        match self.current_stage {
            None => "".into(),
            Some(ref tuple) => {
                match &tuple.1.name {
                    // return the actual name if we have it
                    Some(name) => format!("{:?}", name),
                    // otherwise a number of the stage index
                    None => format!("{:?}", tuple.0),
                }
            }
        }
    }

    pub fn is_paused(&self) -> bool { self.paused }

    /// this just sets a flag that notifies the ProgressItem once its done
    /// whether or not it should run the next stage. it is not possible to actually
    /// stop execution of a stage itself, but we can prevent future stages from running.
    pub fn pause(&mut self) {
        self.paused = true;
    }

    pub fn toggle_pause(&mut self) {
        if self.paused {
            self.resume();
        } else {
            self.pause();
        }
    }

    pub fn resume(&mut self) {
        if !self.paused {
            return;
        }
        // if we are already on a stage and the user calls unpause
        // that is to be treated as simply changing a flag.
        if self.processing_stage {
            self.paused = false;
            return;
        }
        // however, if we are already in a paused state where we
        // are not processing anything, then we want to resume progress
        // by running the next stage in the queue.
        if let Some(task) = self.pause_resume.pop_front() {
            tokio::spawn(async move {
                task.await;
            });
        }
        self.paused = false;
    }

    /// can only pass stages to the progress item if
    /// it has not started yet
    pub fn register_stage<T: Into<Stage>>(&mut self, stage: T) {
        if !self.started {
            self.stages.push_back(stage.into());
        }
    }

    pub fn start<K: Eq + Hash + Debug + Send>(
        &mut self,
        key: K,
        holder: &'static Mutex<ProgressHolder<K>>,
    ) {
        if self.started { return; }
        self.started = true;
        self.numstages = self.stages.len();
        self.do_stage(key, holder, 0);
    }

    pub fn do_stage<K: Eq + Hash + Debug + Send + 'static>(
        &mut self,
        key: K,
        holder: &'static Mutex<ProgressHolder<K>>,
        stage_index: usize,
    ) {
        if stage_index >= self.numstages {
            self.done = true;
            return;
        }
        let max_lock_attempts = self.max_lock_attempts;
        let lock_attempt_wait = self.lock_attempt_wait;

        // if we did a stage before this one, then
        // self.current_stage should be set, so we add that
        // to the past stage list
        if let Some((index, stage)) = &self.current_stage {
            let past_stage_name = match stage.name {
                Some(ref name) => format!("{:?}", name),
                None => index.to_string(),
            };
            self.past_stage_names.push(past_stage_name);
        }

        if let Some(stage) = self.stages.pop_front() {
            if let Some(task) = stage.task {
                self.current_stage = Some((stage_index, Stage {
                    name: stage.name,
                    task: None,
                }));
                self.set_progress(0);

                tokio::spawn(async move {
                    let task_result = task.await;
                    handle_end_of_stage(
                        key,
                        holder,
                        stage_index,
                        task_result,
                        max_lock_attempts,
                        lock_attempt_wait
                    );
                });
                self.processing_stage = true;
                // this is the desirable path, return here
                return;
            }
        }

        // if we failed to get a stage, or we failed to get a task
        // from that stage, then we will consider that an error
        handle_end_of_stage(
            key,
            holder,
            stage_index,
            Err("Failed to run stage".into()),
            max_lock_attempts,
            lock_attempt_wait,
        );
    }
}

pub fn handle_end_of_stage<K: Eq + Hash + Debug + Send>(
    key: K,
    holder: &'static Mutex<ProgressHolder<K>>,
    stage_index: usize,
    task_result: TaskResult,
    max_lock_attempts: usize,
    lock_attempt_wait: u64,
) {
    tokio::spawn(async move {
        // delay first because otherwise doesnt seem we can build the await
        // state machine :(
        tokio::time::delay_for(Duration::from_millis(lock_attempt_wait)).await;
        // then we try to get a lock
        let mut guard = match holder.try_lock() {
            Err(_) => {
                // if we fail to get a lock, try again by calling this recursively
                let new_lock_attempts = if max_lock_attempts == 0 {
                    max_lock_attempts
                } else if max_lock_attempts - 1 == 0 {
                    // if it would reduce to 0, then stop here otherwise wed
                    // have infinite loop on account of the above condition
                    return;
                } else {
                    max_lock_attempts - 1
                };

                handle_end_of_stage(
                    key,
                    holder,
                    stage_index,
                    task_result,
                    new_lock_attempts,
                    lock_attempt_wait,
                );
                return;
            }
            Ok(guard) => guard,
        };

        // if we got the lock, handle it: if is_error, then set self.errored
        // otherwise process the next stage
        match guard.progresses.get_mut(&key) {
            None => {}, // nothing we can do :shrug:
            Some(me) => match task_result {
                Ok(vars_option) => me_do_next_stage(
                    me,
                    holder,
                    key,
                    stage_index,
                    vars_option
                ),
                Err(error_string) => me_set_error(
                    me,
                    stage_index,
                    error_string
                ),
            }
        }
    });
}

pub fn me_do_next_stage<K: Eq + Hash + Debug + Send>(
    me: &mut ProgressItem,
    holder: &'static Mutex<ProgressHolder<K>>,
    key: K,
    stage_index: usize,
    vars_option: Option<ProgressVars>,
) {
    // if given variables, apply
    // these to me so that future
    // stages can see these vars.
    if let Some(mut vars) = vars_option {
        for (key, value) in vars.drain_vars() {
            me.insert_var(key, value);
        }
    }

    me.processing_stage = false;
    if !me.paused {
        me.do_stage(key, holder, stage_index + 1);
    } else {
        // if we are paused, create a future
        // of what we will eventually do when we get unpaused.
        let asynctask = async move {
            match holder.lock() {
                Err(_) => {}
                Ok(mut guard) => {
                    match guard.progresses.get_mut(&key) {
                        None => {}
                        Some(next_me) => {
                            next_me.do_stage(key, holder, stage_index + 1);
                        }
                    }
                }
            }
        };
        // an internal member used to hold this future for later
        // when we unpause
        me.pause_resume.push_back(Box::pin(asynctask));
    }
}

pub fn me_set_error(
    me: &mut ProgressItem,
    stage_index: usize,
    error_string: String,
) {
    me.processing_stage = false;
    me.errored = Some(ProgressError {
        error_string,
        progress_index: stage_index,
        name: match me.current_stage {
            None => None,
            Some(ref tuple) => match tuple.1.name {
                None => None,
                Some(ref name) => Some(format!("{:?}", name)),
            }
        }
    });
}

/// The ProgressHolder is a simple hashmap of the progress items.
/// It is meant to be used statically globally, and it is passed around within
/// the ProgressItem methods as a mutex.
#[derive(Default)]
pub struct ProgressHolder<K: Eq + Hash + Debug> {
    pub progresses: HashMap<K, ProgressItem>,
}

/// A convenience function to easily access the ProgressItem of
/// interest from within the mutex of the ProgressHolder.
/// takes a callback to use the item referenced by the progress holder
/// if the item is found in the progress holder, calls your provided callback
/// otherwise does nothing
pub fn use_me_from_progress_holder<'a, K: Eq + Hash + Debug>(
    key: &K,
    progholder: &'a Mutex<ProgressHolder<K>>,
    cb: impl FnMut(&mut ProgressItem) + 'a,
) {
    use_me_from_progress_holder_or_error(key, progholder, cb, |_| {});
}

pub enum UseProgressError {
    LockUnavailable,
    KeyNotFound,
}

/// like use_me_from_progress_holder(), but also takes an error callback
/// which will call back with an enum of the error type: either we
/// failed to get a lock on the mutex, or we failed to find the
/// key in the progresses hashmap
pub fn use_me_from_progress_holder_or_error<'a, K: Eq + Hash + Debug>(
    key: &K,
    progholder: &'a Mutex<ProgressHolder<K>>,
    ok_cb: impl FnMut(&mut ProgressItem) + 'a,
    err_cb: impl FnMut(UseProgressError) + 'a,
) {
    let mut mut_ok_cb = ok_cb;
    let mut mut_err_cb = err_cb;
    match progholder.try_lock() {
        Err(_) => {
            mut_err_cb(UseProgressError::LockUnavailable);
        },
        Ok(mut guard) => match guard.progresses.get_mut(key) {
            None => {
                mut_err_cb(UseProgressError::KeyNotFound);
            },
            Some(me) => {
                mut_ok_cb(me);
            }
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures_timer::Delay;
    use lazy_static::lazy_static;
    use tokio::runtime::Runtime;

    async fn download_something(secs: u64) -> TaskResult {
        delay_millis(secs * 1000).await;
        Ok(None)
    }

    async fn delay_millis(millis: u64) {
        let duration = std::time::Duration::from_millis(millis);
        Delay::new(duration).await;
    }

    async fn task_no_args() { }

    fn make_simple_progress_item(wait1: u64, wait2: u64, wait3: u64) -> ProgressItem {
        let future1 = download_something(wait1);
        let future2 = download_something(wait2);
        let future3 = download_something(wait3);
        let mystage1 = Stage::make("wait1", future1);
        let mystage2 = Stage::make("wait2", future2);
        let mystage3 = Stage::make("wait3", future3);
        let mut prog = ProgressItem::new();
        prog.register_stage(mystage1);
        prog.register_stage(mystage2);
        prog.register_stage(mystage3);
        prog
    }

    // these ones will be in millis
    async fn make_advanced_stage<K: Eq + Hash + Debug + Send>(
        wait: u64,
        key: K,
        progholder: &'static Mutex<ProgressHolder<K>>
    ) {
        for i in 1..4 {
            let duration = std::time::Duration::from_millis(wait);
            Delay::new(duration).await;
            match progholder.lock() {
                Err(_) => {},
                Ok(mut guard) => match guard.progresses.get_mut(&key) {
                    None => {}
                    Some(progitem) => {
                        progitem.set_progress(i * 25_000);
                    }
                }
            }
        }
    }

    fn make_advanced_progress_item<K: Eq + Hash + Debug + Send + Clone>(
        wait: u64,
        key: K,
        progholder: &'static Mutex<ProgressHolder<K>>,
    ) -> ProgressItem {
        let stage1 = Stage::make_simple("wait1", make_advanced_stage(wait, key.clone(), progholder));
        let stage2 = Stage::make_simple("wait2", make_advanced_stage(wait, key.clone(), progholder));
        let mut prog = ProgressItem::new();
        prog.register_stage(stage1);
        prog.register_stage(stage2);
        prog
    }

    macro_rules! run_in_tokio_with_static_progholder {
        ($($something:expr;)+) => {
            {
                lazy_static! {
                    static ref PROGHOLDER: Mutex<ProgressHolder<String>> = Mutex::new(
                        ProgressHolder::<String>::default()
                    );
                }
                let mut rt = Runtime::new().unwrap();
                rt.block_on(async move {
                    $(
                        $something;
                    )*
                });
            }
        };
    }

    pub fn get_progress_stage_name(key: &String, progholder: &'static Mutex<ProgressHolder<String>>) -> String {
        match progholder.lock() {
            Err(_) => "ooops".into(),
            Ok(mut guard) => match guard.progresses.get_mut(key) {
                None => "oops".into(),
                Some(progitem) => progitem.get_stage_name(),
            },
        }
    }

    pub fn get_progress_percent(key: &String, progholder: &'static Mutex<ProgressHolder<String>>) -> Option<u32> {
        let mut guard = progholder.lock().unwrap();
        match guard.progresses.get_mut(key) {
            None => None,
            Some(ref progitem) => {
                Some(progitem.get_progress())
            }
        }
    }

    #[test]
    fn progress_can_set_and_view_vars() {
        run_in_tokio_with_static_progholder! {{
            let key = String::from("key");
            // stage1 will return some variables
            // then stage2 will try to read them
            let stage1 = Stage::make("wait1", async {
                delay_millis(1000).await;
                let mut vars = ProgressVars::default();
                vars.insert_var("testkey1", Box::new("testvalue"));
                vars.insert_var("testkey2", Box::new(100));
                Ok(Some(vars))
            });
            let stage2 = Stage::make("wait2", async {
                use_me_from_progress_holder_or_error(&String::from("key"), &PROGHOLDER, |me| {
                    assert!(me.var_exists("testkey1"));
                    assert!(me.var_exists("testkey2"));
                    // TODO: check if you can downcast...
                    let boxstr = me.extract_var("testkey1").unwrap();
                    let boxstr = boxstr.downcast::<&str>().unwrap();
                    assert_eq!(*boxstr, "testvalue");

                    let boxint = me.extract_var("testkey2").unwrap();
                    let boxint = boxint.downcast::<i32>().unwrap();
                    assert_eq!(*boxint, 100);
                }, |_| {
                    assert!(false);
                });
                Ok(None)
            });
            let mut myprog = ProgressItem::new();
            myprog.register_stage(stage1);
            myprog.register_stage(stage2);
            let mut myprog = myprog.set_lock_attempt_duration(0);
            let mut guard = PROGHOLDER.lock().unwrap();
            myprog.start(key.clone(), &PROGHOLDER);
            guard.progresses.insert(key.clone(), myprog);
            drop(guard);

            // first check that progress item should not contain
            // the vars created by stage1 because stage1
            // hasnt finished yet
            use_me_from_progress_holder_or_error(&key, &PROGHOLDER, |me| {
                assert!(!me.var_exists("testkey1"));
                assert!(!me.var_exists("testkey2"));
            }, |_| {
                assert!(false);
            });

            // then wait until stage1 finishes, the rest of
            // the test is done by stage2 when it checks
            // if the vars exist
            delay_millis(1010).await;
        };};
    }

    #[test]
    fn can_get_stage_view_vec() {
        run_in_tokio_with_static_progholder! {{
            let key = String::from("key");
            let wait = 100; // millis. each advanced stage does wait * 4
            let stage1 = Stage::make_simple("wait1", make_advanced_stage(wait, key.clone(), &PROGHOLDER));
            let stage2 = Stage::make_simple("wait2", make_advanced_stage(wait, key.clone(), &PROGHOLDER));
            let stage3 = Stage::make_simple("wait3", make_advanced_stage(wait, key.clone(), &PROGHOLDER));
            let stage4 = Stage::make_simple("wait4", async {
                delay_millis(1000).await;
            });
            let mut prog = ProgressItem::new();
            prog.register_stage(stage1);
            prog.register_stage(stage2);
            prog.register_stage(stage3);
            prog.register_stage(stage4);

            let mut prog = prog.set_lock_attempt_duration(0);
            let mut guard = PROGHOLDER.lock().unwrap();
            prog.start(key.clone(), &PROGHOLDER);
            guard.progresses.insert(key.clone(), prog);
            drop(guard);

            // let it run until its somewhere in the middle of stage 2
            delay_millis(wait * 4 + 10).await;
            use_me_from_progress_holder_or_error(&key, &PROGHOLDER, |me| {
                let progress_name = me.get_stage_name();
                assert!(progress_name.contains("wait2"));
                // now here we get its stage view
                let stage_view_vec: Vec<StageView> = me.into();
                assert_eq!(stage_view_vec.len(), 4);
                let first = &stage_view_vec[0];
                let second = &stage_view_vec[1];
                let third = &stage_view_vec[2];

                assert_eq!(first.index, 0);
                assert_eq!(first.progress_percent, 100.0);
                assert!(first.name.contains("wait1"));
                assert!(!first.currently_processing);

                // check if its actually somewhere
                // in the middle of its progress
                assert!(second.progress_percent > 0.0);
                assert!(second.progress_percent < 100.0);
                assert!(second.currently_processing);
                assert_eq!(second.index, 1);

                assert_eq!(third.progress_percent, 0.0);
                assert!(third.name.contains("wait3"));
                assert_eq!(third.index, 2);
                assert!(!third.currently_processing);
            }, |_| {
                assert!(false);
            });

            // if we wait until first 3 stages are done
            // the last stage should be at 0% at first
            // while its processing (and it never updates itself)
            delay_millis(wait * 4 * 2).await;
            use_me_from_progress_holder_or_error(&key, &PROGHOLDER, |me| {
                let stage_view_vec: Vec<StageView> = me.into();
                let last = &stage_view_vec[3];
                assert_eq!(last.progress_percent, 0.0);
                assert!(last.name.contains("wait4"));
                assert_eq!(last.index, 3);
                assert!(last.currently_processing);
            }, |_| {
                assert!(false);
            });

            // then we wait for the last stage to finish, and it
            // should have a progress of 100.0 implicitly
            delay_millis(1000).await;
            use_me_from_progress_holder_or_error(&key, &PROGHOLDER, |me| {
                let stage_view_vec: Vec<StageView> = me.into();
                let last = &stage_view_vec[3];
                assert_eq!(last.progress_percent, 100.0);
                assert!(last.name.contains("wait4"));
                assert_eq!(last.index, 3);
                assert!(!last.currently_processing);
            }, |_| {
                assert!(false);
            })
        };};
    }

    #[test]
    fn can_pause_progress() {
        run_in_tokio_with_static_progholder! {{
            let key = String::from("key");
            let myprog = make_advanced_progress_item(100, key.clone(), &PROGHOLDER);
            let mut myprog = myprog.set_lock_attempt_duration(0);
            let mut guard = PROGHOLDER.lock().unwrap();
            myprog.start(key.clone(), &PROGHOLDER);
            guard.progresses.insert(key.clone(), myprog);
            drop(guard);


            // we should still be on the first progres stage "wait1"
            delay_millis(100 * 3).await;
            use_me_from_progress_holder_or_error(&key, &PROGHOLDER, |me| {
                let progress_name = me.get_stage_name();
                assert!(progress_name.contains("wait1"));
                // here we should be able to pause it
                // and if we wait another few seconds
                // we should still be on stage wait1
                // because we never progressed to wait2
                me.pause();
            }, |_| {
                assert!(false);
            });

            // so now if we wait another 300ms, we normally would have
            // been on the next stage: wait2, but since we paused it
            // we should still be on wait1...
            delay_millis(300).await;
            use_me_from_progress_holder_or_error(&key, &PROGHOLDER, |me| {
                let progress_name = me.get_stage_name();
                assert!(progress_name.contains("wait1"));
            }, |_| {
                assert!(false);
            });

            // then we can also resume progress and check again to see
            // that it should have gone to the next stage
            use_me_from_progress_holder_or_error(&key, &PROGHOLDER, |me| {
                me.resume();
            }, |_| {
                assert!(false);
            });

            delay_millis(100).await;
            use_me_from_progress_holder_or_error(&key, &PROGHOLDER, |me| {
                let progress_name = me.get_stage_name();
                assert!(progress_name.contains("wait2"));
            }, |_| {
                assert!(false);
            });
        };};
    }

    #[test]
    fn inc_progress_percent_works() {
        let mut myprog = ProgressItem::new();
        myprog.inc_progress_percent(2.0);
        assert_eq!(myprog.get_progress_percent(), 2.0);

        // it shouldnt allow it to go down
        myprog.inc_progress_percent(1.0);
        assert_eq!(myprog.get_progress_percent(), 2.0);
    }

    #[test]
    fn using_from_callback_works() {
        run_in_tokio_with_static_progholder! {{
            let key = String::from("key");
            let myprog = make_advanced_progress_item(250, key.clone(), &PROGHOLDER);
            let myprog = myprog.set_lock_attempt_duration(0);
            let mut guard = PROGHOLDER.lock().unwrap();
            guard.progresses.insert(key.clone(), myprog);
            drop(guard);

            use_me_from_progress_holder(&key, &PROGHOLDER, |me| {
                assert_eq!(me.get_progress(), 0);
                me.set_progress(55);
            });

            match PROGHOLDER.lock() {
                Err(_) => assert!(false),
                Ok(mut guard) => match guard.progresses.get_mut(&key) {
                    None => assert!(false),
                    Some(me) => assert_eq!(me.get_progress(), 55),
                }
            }
        };};
    }

    #[test]
    fn get_and_set_progress_percent_works() {
        let mut myprog = ProgressItem::new();
        myprog.set_progress_percent(0.001);
        assert_eq!(myprog.get_progress_percent(), 0.001);

        myprog.set_progress_percent(50.0);
        assert_eq!(myprog.get_progress_percent(), 50.0);
    }

    #[test]
    fn get_and_set_progress_percent_normalized_works() {
        let mut myprog = ProgressItem::new();
        myprog.set_progress_percent_normalized(0.5);
        assert_eq!(myprog.get_progress_percent_normalized(), 0.5);

        myprog.set_progress_percent_normalized(0.9999);
        assert_eq!(myprog.get_progress_percent_normalized(), 0.9999);
    }

    #[test]
    fn can_update_progress_value() {
        run_in_tokio_with_static_progholder! {{
            let key = String::from("key");
            let myprog = make_advanced_progress_item(250, key.clone(), &PROGHOLDER);
            let mut myprog = myprog.set_lock_attempt_duration(0);
            let mut guard = PROGHOLDER.lock().unwrap();
            myprog.start(key.clone(), &PROGHOLDER);
            guard.progresses.insert(key.clone(), myprog);
            drop(guard);

            // we should start at 0%
            delay_millis(10).await;
            let progress = get_progress_percent(&key, &PROGHOLDER).unwrap();
            assert_eq!(progress, 0);

            // after a half seconds it should be more than 0
            // but we should still be in stage 1
            delay_millis(500).await;
            let progress = get_progress_percent(&key, &PROGHOLDER).unwrap();
            let progress_name = get_progress_stage_name(&key, &PROGHOLDER);
            assert!(progress > 0);
            assert!(progress_name.contains("wait1"));

            // after another half second, we should be again greater than 0
            // but in the next stage
            delay_millis(600).await;
            let progress = get_progress_percent(&key, &PROGHOLDER).unwrap();
            let progress_name = get_progress_stage_name(&key, &PROGHOLDER);
            assert!(progress > 0);
            assert!(progress_name.contains("wait2"));
        };};
    }

    #[test]
    fn should_auto_done_if_no_stages_provided() {
        run_in_tokio_with_static_progholder! {{
            let mut myprog = ProgressItem::new();
            assert!(!myprog.is_done());
            myprog.start("a".into(), &PROGHOLDER);
            assert!(myprog.is_done());
        };};
    }

    #[test]
    fn should_error_if_cant_get_task_in_stage() {
        run_in_tokio_with_static_progholder! {{
            let myprog = ProgressItem::new();
            let mut myprog = myprog.set_lock_attempt_duration(0);
            let mystage = Stage::new("a"); // no task here. should error
            myprog.register_stage(mystage);
            assert!(!myprog.is_done());

            let key = String::from("reee");
            let mut guard = PROGHOLDER.lock().unwrap();
            myprog.start(key.clone(), &PROGHOLDER);
            guard.progresses.insert(key.clone(), myprog);
            drop(guard);

            delay_millis(10).await;
            let mut guard = PROGHOLDER.lock().unwrap();
            match guard.progresses.get_mut(&key) {
                None => assert!(false),
                Some(ref progitem) => match progitem.get_progress_error() {
                    None => assert!(false),
                    Some(err) => assert!(err.error_string.contains("Failed to run stage")),
                }
            }
        };};
    }

    #[test]
    fn get_stage_name_works() {
        run_in_tokio_with_static_progholder! {{
            let myprog = make_simple_progress_item(1, 1, 1);
            let mut myprog = myprog.set_lock_attempt_duration(0);
            let key: String = "reee".into();
            match PROGHOLDER.lock() {
                Err(_) => {},
                Ok(mut guard) => {
                    myprog.start(key.clone(), &PROGHOLDER);
                    guard.progresses.insert(key.clone(), myprog);
                },
            }

            assert!(get_progress_stage_name(&key, &PROGHOLDER).contains("wait1"));
            delay_millis(1100).await;
            assert!(get_progress_stage_name(&key, &PROGHOLDER).contains("wait2"));
            delay_millis(1100).await;
            assert!(get_progress_stage_name(&key, &PROGHOLDER).contains("wait3"));
        };};
    }

    #[test]
    fn can_call_start() {
        run_in_tokio_with_static_progholder! {{
            let future = download_something(3);
            let mystage = Stage::make("download_something", future);
            let mut myprogitem = ProgressItem::new();
            myprogitem.register_stage(mystage);
            myprogitem.start(String::from("reeeee"), &PROGHOLDER);
        };};
    }

    #[test]
    fn can_easily_create_a_stage() {
        let future = download_something(3);
        let _ = Stage::make("download_something", future);
    }

    #[test]
    fn can_easily_create_a_stage_and_add_register_to_progress() {
        let future = download_something(3);
        let mystage = Stage::make("download_something", future);
        let mut myprogitem = ProgressItem::new();
        myprogitem.register_stage(mystage);
    }

    #[test]
    fn can_make_a_stage_from_enum() {
        #[derive(Debug)]
        pub enum ThisEnum {
            Download3,
            Download6,
        }
        let future3 = download_something(3);
        let future6 = download_something(6);
        let done = async { };
        let mystage3 = Stage::make(ThisEnum::Download3, future3);
        let mystage6 = Stage::make(ThisEnum::Download6, future6);
        // the name of the stage is put into a box, so it doesnt all
        // have to be of the same type
        let mystagedone = Stage::make_simple("DONE!", done);

        // note the name of the progress item is a string
        // even though the name of the tasks are enums. this is ok
        // as long as all tasks in this progress item also have names as enums
        let mut myprogitem = ProgressItem::new();
        myprogitem.register_stage(mystage3);
        myprogitem.register_stage(mystage6);
        myprogitem.register_stage(mystagedone);
    }

    #[test]
    fn make_stage_macros_work() {
        let mystage1 = make_stage!(download_something; 2);
        match mystage1.name {
            None => assert!(false),
            Some(name) => {
                let name_string = format!("{:?}", name);
                assert!(name_string.contains("download_something"));
            }
        }

        let mystage2 = make_stage_simple!(delay_millis; 2);
        match mystage2.name {
            None => assert!(false),
            Some(name) => {
                let name_string = format!("{:?}", name);
                assert!(name_string.contains("delay_millis"));
            }
        }

        let mystage3 = make_stage_simple!(task_no_args; );
        match mystage3.name {
            None => assert!(false),
            Some(name) => {
                let name_string = format!("{:?}", name);
                assert!(name_string.contains("task_no_args"));
            }
        }
    }
}
