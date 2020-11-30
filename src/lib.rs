use std::fmt::Debug;
use std::fmt::Display;
use std::future::Future;
use std::collections::VecDeque;
use std::collections::HashMap;
use std::pin::Pin;
use futures::task;
use std::task::{Context, Poll};


type TaskResult = Result<(), String>;
type PinBoxFuture = Pin<Box<dyn Future<Output = TaskResult> + Send>>;

pub struct Stage {
    name: Option<Box<dyn Debug>>,
    task: Option<PinBoxFuture>,
}

impl Stage {
    pub fn new(name: impl Debug + 'static) -> Self {
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
        name: impl Debug + 'static,
        future: F,
    ) -> Self {
        Stage::new(name).set_task_from_future(future)
    }

    pub fn make_simple<F: Future<Output = ()> + Send + 'static>(
        name: impl Debug + 'static,
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

#[derive(Debug, Default)]
pub struct ProgressItem<S: Debug> {
    name: S,
    stages: VecDeque<Stage>,
    started: bool,
}

impl<S: Debug> ProgressItem<S> {
    pub fn new(name: S) -> Self {
        ProgressItem {
            name,
            stages: VecDeque::new(),
            started: false,
        }
    }

    pub fn register_stage<T: Into<Stage>>(&mut self, stage: T) {
        if !self.started {
            self.stages.push_back(stage.into());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures_timer::Delay;

    async fn download_something(secs: u64) -> TaskResult {
        let duration = std::time::Duration::from_secs(secs);
        Delay::new(duration).await;

        Ok(())
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
