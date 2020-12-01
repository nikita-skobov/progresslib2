use std::fmt::Debug;
use std::hash::Hash;
use std::fmt::Display;
use std::future::Future;
use std::sync::{MutexGuard, Mutex};
use std::collections::VecDeque;
use std::collections::HashMap;
use std::pin::Pin;
use futures::task;
use std::task::{Context, Poll};


type TaskResult = Result<(), String>;
type PinBoxFuture = Pin<Box<dyn Future<Output = TaskResult> + Send>>;

pub struct Stage {
    pub name: Option<Box<dyn Debug + Send>>,
    task: Option<PinBoxFuture>,
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
            Ok(())
        }));
        self
    }

    pub fn make<F: Future<Output = TaskResult> + Send + 'static>(
        name: impl Debug + Send + 'static,
        future: F,
    ) -> Self {
        Stage::new(name).set_task_from_future(future)
    }

    pub fn make_simple<F: Future<Output = ()> + Send + 'static>(
        name: impl Debug + Send + 'static,
        future: F,
    ) -> Self {
        Stage::new(name).set_task_from_future(async move {
            future.await;
            Ok(())
        })
    }
}

pub const MAX_PROGRESS_TICKS: u32 = 100_000;
pub const TICKS_PER_PERCENT: u32 = MAX_PROGRESS_TICKS / 100;

#[derive(Debug)]
pub struct ProgressError {
    name: Option<String>,
    progress_index: usize,
    error_string: String,
}

#[derive(Default)]
pub struct ProgressItem<S: Debug> {
    name: S,
    stages: VecDeque<Stage>,
    started: bool,
    numstages: usize,
    current_stage: Option<(usize, Stage)>,
    progress: u32, // 0 - 100,000 (each 1,000 is 1%)
    errored: Option<ProgressError>,
    done: bool,
}


impl<S: Debug + Send> ProgressItem<S> {
    pub fn new(name: S) -> Self {
        ProgressItem {
            name,
            numstages: 0,
            stages: VecDeque::new(),
            started: false,
            current_stage: None,
            errored: None,
            done: false,
            progress: 0,
        }
    }

    /// set the progress level. new_progress must be in 'ticks'
    /// where 1000 ticks represents 1%
    pub fn set_progress(&mut self, new_progress: u32) {
        let overflow_check: u64 = self.progress as u64 + new_progress as u64;
        if overflow_check > MAX_PROGRESS_TICKS as u64 {
            self.progress = MAX_PROGRESS_TICKS;
        } else {
            // this is safe to do because we checked if its over 100,000 which if its not
            // then it will definitely fit into u32
            self.progress = overflow_check as u32;
        }
    }

    /// like set_progress but only allows progress to increase
    pub fn inc_progress(&mut self, new_progress: u32) {
        if new_progress < self.progress {
            self.set_progress(new_progress);
        }
    }

    pub fn get_progress(&self) -> u32 { self.progress }

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

    pub fn register_stage<T: Into<Stage>>(&mut self, stage: T) {
        if !self.started {
            self.stages.push_back(stage.into());
        }
    }

    pub fn start<K: Eq + Hash + Debug + Send>(
        &mut self,
        key: K,
        holder: &'static Mutex<ProgressHolder<K, S>>,
    ) {
        if self.started { return; }
        self.started = true;
        self.numstages = self.stages.len();
        self.do_stage(key, holder, 0);
    }

    pub fn do_stage<K: Eq + Hash + Debug + Send + 'static>(
        &mut self,
        key: K,
        holder: &'static Mutex<ProgressHolder<K, S>>,
        stage_index: usize,
    ) {
        if stage_index >= self.numstages {
            self.done = true;
            return;
        }
        if let Some(stage) = self.stages.pop_front() {
            if let Some(task) = stage.task {
                self.current_stage = Some((stage_index, Stage {
                    name: stage.name,
                    task: None,
                }));
                self.progress = 0;
                tokio::spawn(async move {
                    let task_result = task.await;
                    match task_result {
                        Ok(_) => Self::handle_ok(key, holder, stage_index),
                        Err(s) => Self::handle_error(key, holder, stage_index, s),
                    };
                });

                // this is the desirable path, return here
                return;
            }
        }
        // if we failed to get a stage, or we failed to get a task
        // from that stage, then we will consider that an error
        Self::handle_error(key, holder, stage_index, "Failed to run stage".into());
    }

    pub fn handle_ok<K: Eq + Hash + Debug + Send>(
        key: K,
        holder: &'static Mutex<ProgressHolder<K, S>>,
        stage_index: usize,
    ) {
        match holder.lock() {
            Err(_) => {}
            Ok(mut guard) => match guard.progresses.get_mut(&key) {
                None => {}
                Some(me) => {
                    me.do_stage(key, holder, stage_index + 1);
                }
            }
        }
    }

    pub fn handle_error<K: Eq + Hash + Debug>(
        key: K,
        holder: &'static Mutex<ProgressHolder<K, S>>,
        stage_index: usize,
        error_string: String,
    ) {
        match holder.lock() {
            Err(_) => {}
            Ok(mut guard) => match guard.progresses.get_mut(&key) {
                None => {}
                Some(mut me) => {
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
            }
        }
    }
}

#[derive(Default)]
pub struct ProgressHolder<K: Eq + Hash + Debug, S: Debug> {
    pub progresses: HashMap<K, ProgressItem<S>>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures_timer::Delay;
    use lazy_static::lazy_static;
    use tokio::prelude::*;
    use tokio::runtime::Runtime;

    async fn download_something(secs: u64) -> TaskResult {
        delay_millis(secs * 1000).await;
        Ok(())
    }

    async fn delay_millis(millis: u64) {
        let duration = std::time::Duration::from_millis(millis);
        Delay::new(duration).await;
    }

    fn make_simple_progress_item(wait1: u64, wait2: u64, wait3: u64) -> ProgressItem<&'static str> {
        let future1 = download_something(wait1);
        let future2 = download_something(wait2);
        let future3 = download_something(wait3);
        let mystage1 = Stage::make("wait1", future1);
        let mystage2 = Stage::make("wait2", future2);
        let mystage3 = Stage::make("wait3", future3);
        let mut prog = ProgressItem::new("simple");
        prog.register_stage(mystage1);
        prog.register_stage(mystage2);
        prog.register_stage(mystage3);
        prog
    }

    macro_rules! run_in_tokio_with_static_progholder {
        ($($something:expr;)+) => {
            {
                lazy_static! {
                    static ref PROGHOLDER: Mutex<ProgressHolder<String, &'static str>> = Mutex::new(
                        ProgressHolder::<String, &'static str>::default()
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

    pub fn get_progress_stage_name(key: &String, progholder: &'static Mutex<ProgressHolder<String, &'static str>>) -> String {
        match progholder.lock() {
            Err(_) => "ooops".into(),
            Ok(mut guard) => match guard.progresses.get_mut(key) {
                None => "oops".into(),
                Some(progitem) => progitem.get_stage_name(),
            },
        }
    }

    #[test]
    fn get_stage_name_works() {
        run_in_tokio_with_static_progholder! {{
            let mut myprog = make_simple_progress_item(1, 1, 1);
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
            let mut myprogitem = ProgressItem::new("ayyy");
            myprogitem.register_stage(mystage);
            myprogitem.start(String::from("reeeee"), &PROGHOLDER);
        };};
    }

    #[test]
    fn can_easily_create_a_stage() {
        let future = download_something(3);
        let mystage = Stage::make("download_something", future);
    }

    #[test]
    fn can_easily_create_a_stage_and_add_register_to_progress() {
        let future = download_something(3);
        let mystage = Stage::make("download_something", future);
        let mut myprogitem = ProgressItem::new("ayyy");
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
        let mut myprogitem = ProgressItem::new("ayy");
        myprogitem.register_stage(mystage3);
        myprogitem.register_stage(mystage6);
        myprogitem.register_stage(mystagedone);
    }
}
