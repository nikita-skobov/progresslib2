use std::fmt::Debug;
use std::fmt::Display;
use std::future::Future;
use std::collections::VecDeque;
use std::collections::HashMap;
use std::pin::Pin;
use futures::task;
use std::task::{Context, Poll};


type PinBoxFuture = Pin<Box<dyn Future<Output = ()> + Send>>;

pub struct Stage<S: Debug> {
    name: S,
    task: Option<PinBoxFuture>,
}

impl<S: Debug> Stage<S> {
    pub fn new(name: S) -> Self {
        Stage {
            name,
            task: None,
        }
    }
    pub fn set_task_from_future<F: Future<Output = ()> + Send + 'static>(
        mut self,
        future: F
    ) -> Self {
        self.task = Some(Box::pin(future));
        self
    }

    pub fn make<F: Future<Output = ()> + Send + 'static>(
        name: S,
        future: F,
    ) -> Self {
        Stage::new(name).set_task_from_future(future)
    }
}

impl<S: Debug> Debug for Stage<S> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Stage")
            .field("name", &self.name)
            .field("future", match &self.task {
                Some(_) => &"Some future",
                None => &"None",
            })
            .finish()
    }
}

#[derive(Debug, Default)]
pub struct ProgressItem<S1: Debug, S2: Debug> {
    name: S1,
    stages: VecDeque<Stage<S2>>,
    started: bool,
}

impl<S1: Debug, S2: Debug> ProgressItem<S1, S2> {
    pub fn new(name: S1) -> Self {
        ProgressItem {
            name,
            stages: VecDeque::new(),
            started: false,
        }
    }

    pub fn register_stage<T: Into<Stage<S2>>>(&mut self, stage: T) {
        if !self.started {
            self.stages.push_back(stage.into());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures_timer::Delay;

    async fn download_something(secs: u64) {
        let duration = std::time::Duration::from_secs(secs);
        Delay::new(duration).await;
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
            DownloadDone,
        }
        let future3 = download_something(3);
        let future6 = download_something(6);
        let done = async { };
        let mystage3 = Stage::make(ThisEnum::Download3, future3);
        let mystage6 = Stage::make(ThisEnum::Download6, future6);
        let mystagedone = Stage::make(ThisEnum::DownloadDone, done);

        // note the name of the progress item is a string
        // even though the name of the tasks are enums. this is ok
        // as long as all tasks in this progress item also have names as enums
        let mut myprogitem = ProgressItem::new("ayy");
        myprogitem.register_stage(mystage3);
        myprogitem.register_stage(mystage6);
        myprogitem.register_stage(mystagedone);
    }
}
