# Progresslib2

It technically should be called Progresslib99999 because I made many prototypes of this before settling on this design.

This library is intended to abstract the idea of keeping track of various progresses in an application.

This library lets you define progress workflows in the following way:

- A queue of stages, where each stage starts and ends implicitly at 0% and 100% respectively.
- Within a stage is a task that can optionally modify the progress value from 0 to 100%. This modification is purely computational and does not affect the stages (ie: setting the progress value to 100% does not end the stage).
- Once a stage is done, the progress value is reset to 0, and the next stage is taken off of the queue and processed.

It works with the following abstractions:

- A `Stage` is anything that holds a [future](https://doc.rust-lang.org/std/future/trait.Future.html). By making a `Stage` based around a future, it allows you to easily create any kind of stage you want in the following way:
    ```rs
    pub async fn transfer(src: Thing, dest: Thing) {
        // some asynchronous code
    }
    let mystage = Stage::make_simple("mystage", transfer(a, b));
    ```
    a stage also holds a name. As seen here, the name we give is "mystage". But the name can be anything that implements `Debug`. This means it is a good pattern to design stages such as:
    ```rs
    #[derive(Debug)]
    pub enum MyStages {
        Stage1,
        Stage2,
        Stage3,
    }
    pub async fn something() {}
    fn make_stages() {
        let s1 = Stage::make_simple(MyStages::Stage1, something());
        let s2 = Stage::make_simple(MyStages::Stage2, something());
        let s3 = Stage::make_simple(MyStages::Stage3, something());
    }
    ```
- One or more `Stage`s are held by a `ProgressItem`. A `ProgressItem` is the core part of this library. it contains a queue of `Stage`s that it runs through, and for each `Stage`, it executes the task asynchronously via `tokio`. When the asynchronous task is done, it has a reference to the global `ProgressHolder` such that it can "call itself" by looking itself up in the `ProgressHolder` via its key. This allows it to be non-blocking in the sense that it only needs to acquire a lock of itself when updating its basic fields, but when it runs the `Stage`, it does not need to lock the mutex of the `ProgressHolder`. See the `src/lib.rs` for more detailed information, or see `examples/youtubedl.rs` for a simple server implementation.

- Lastly, there is `ProgressHolder` which is the simplest of the three. It is just a `HashMap` of the `ProgressItems`. This library is designed around having a single global static `ProgressHolder` that is then passed around as a mutex in the various `ProgressItem` methods. This allows progress to be updated asynchronously and without keeping a lock open while updating progress.


Here is a really simple example, but for a more real-world example, see [`examples/youtubedl.rs`](examples/youtubedl.rs).

```rs
use progresslib2::*;
use lazy_static::lazy_static;
use std::sync::Mutex;

lazy_static! {
    static ref PROGHOLDER: Mutex<ProgressHolder<String>> = Mutex::new(
        ProgressHolder::<String>::default()
    );
}

pub async fn handle_data_in_background(data: WebRequestData, key: String) {
    // here we might actually want to modify the value
    // of the progress as we do our data processing
    use_me_from_progress_holder(&key, &PROGHOLDER, |me| {
        me.set_progress_percent(50 as f64);
    });
    // do something asynchronous ...
    use_me_from_progress_holder(&key, &PROGHOLDER, |me| {
        me.set_progress_percent(90 as f64);
    });
}

pub async fn finalize_data_in_background(data: WebRequestData) {
    // this stage does not update the progress, but rather
    // starts at 0%, and when this function returns, the stage
    // is considered done at 100%.
}

pub fn make_progress_item(data: WebRequestData, key: String) -> ProgressItem {
    let stage1 = Stage::make_simple("handle_data", handle_data_in_background(data.clone()), key.clone());
    let stage2 = Stage::make_simple("finalize_data", finalize_data_in_background(data.clone()));
    let mut progitem = ProgressItem::new();
    progitem.register_stage(stage1);
    progitem.register_stage(stage2);
    progitem
}

pub fn web_request(data: WebRequestData) {
    // some key that will reference the progress item
    let key = String::from("key");
    // make_progress_item is like a template builder
    // it returns a specific type of progress item that we have defined
    // for our application. the difference is that the actual parameters
    // of this progress item are dynamic and determined by whatever is received
    // by this web request
    let my_progress = make_progress_item(data, key.clone());

    let mut mutex_guard = PROGHOLDER.lock().unwrap();
    my_progress.start(key.clone(), &PROGHOLDER);
    mutex_guard.insert(key, my_progress);
    
    // really important to drop this guard early
    // especially if we have a lot of computation afterwards.
    drop(mutex_guard);
    // but in this example it would go out of scope here anyway
}
```
