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

impl Debug for Stage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Stage")
            .field("name", &self.name)
            .field("future", match &self.task {
                Some(_) => &"set",
                None => &"not set",
            })
            .finish()
    }
}

#[derive(Debug)]
pub struct ProgressError {
    name: Option<String>,
    progress_index: usize,
    error_string: String,
}

#[derive(Debug, Default)]
pub struct ProgressItem<S: Debug> {
    name: S,
    stages: VecDeque<Stage>,
    started: bool,
    numstages: usize,
    current_stage: Option<(usize, Stage)>,
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
        // TODO: self.errored
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

    async fn download_something(secs: u64) -> TaskResult {
        let duration = std::time::Duration::from_secs(secs);
        Delay::new(duration).await;

        Ok(())
    }

    // TODO: instantiate tokio otherwise start will fail.
    #[test]
    fn can_call_start() {
        let future = download_something(3);
        let mystage = Stage::make("download_something", future);
        let mut myprogitem = ProgressItem::new("ayyy");
        myprogitem.register_stage(mystage);

        lazy_static! {
            static ref PROGHOLDER: Mutex<ProgressHolder<String, &'static str>> = Mutex::new(
                ProgressHolder::<String, &'static str>::default()
            );
        }

        myprogitem.start(String::from("reeeee"), &PROGHOLDER);
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
