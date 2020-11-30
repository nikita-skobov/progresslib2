use std::fmt::Debug;
use std::fmt::Display;
use std::future::Future;
use std::collections::VecDeque;
use std::collections::HashMap;
use std::pin::Pin;
use futures::task;
use std::task::{Context, Poll};


type PinBoxFuture = Pin<Box<dyn Future<Output = ()> + Send>>;

pub struct Task<S: Debug> {
    name: S,
    task: Option<PinBoxFuture>,
}

impl<S: Debug> Task<S> {
    pub fn new(name: S) -> Self {
        Task {
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
        Task::new(name).set_task_from_future(future)
    }
}

impl<S: Debug> Debug for Task<S> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Task")
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
    tasks: VecDeque<Task<S2>>,
    started: bool,
}

impl<S1: Debug, S2: Debug> ProgressItem<S1, S2> {
    pub fn new(name: S1) -> Self {
        ProgressItem {
            name,
            tasks: VecDeque::new(),
            started: false,
        }
    }

    pub fn register_task<T: Into<Task<S2>>>(&mut self, task: T) {
        if !self.started {
            self.tasks.push_back(task.into());
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
    fn can_easily_create_a_task() {
        let future = download_something(3);
        let mytask = Task::make("download_something", future);
    }

    #[test]
    fn can_easily_create_a_task_and_add_register_to_progress() {
        let future = download_something(3);
        let mytask = Task::make("download_something", future);
        let mut myprogitem = ProgressItem::new("ayyy");
        myprogitem.register_task(mytask);
    }

    fn can_make_a_task_from_enum() {
        #[derive(Debug)]
        pub enum ThisEnum {
            Download3,
            Download6,
            DownloadDone,
        }
        let future3 = download_something(3);
        let future6 = download_something(6);
        let done = async { };
        let mytask3 = Task::make(ThisEnum::Download3, future3);
        let mytask6 = Task::make(ThisEnum::Download6, future6);
        let mytaskdone = Task::make(ThisEnum::DownloadDone, done);

        // note the name of the progress item is a string
        // even though the name of the tasks are enums. this is ok
        // as long as all tasks in this progress item also have names as enums
        let mut myprogitem = ProgressItem::new("ayy");
        myprogitem.register_task(mytask3);
        myprogitem.register_task(mytask6);
        myprogitem.register_task(mytaskdone);
    }
}
