use actix_web::web;
use actix_web::HttpServer;
use actix_web::HttpResponse;
use actix_web::App;
use lazy_static::lazy_static;
use std::sync::Mutex;
use progresslib2::*;
use serde::{Deserialize, Serialize};
use tokio::process::Command;
use tokio::io::{BufReader, AsyncBufReadExt};
use std::process::Stdio;


lazy_static! {
    static ref PROGHOLDER: Mutex<ProgressHolder<String>> = Mutex::new(
        ProgressHolder::<String>::default()
    );
}

/// reads a given line from the output of youtube-dl
/// and parses it (very roughly and not perfectly)
/// and returns the value from 0 - 100, or None if it failed to parse
pub fn get_ytdl_progress(line: &str) -> Option<u8> {
    let mut ret_value = None;

    let percent_index = line.find('%');
    if let Some(percent_index) = percent_index {
        if line.contains("[download]") {
            let mut prev_index = percent_index;
            while line.get((prev_index - 1)..prev_index) != Some(" ") {
                prev_index -= 1;
                if prev_index == 0 {
                    break;
                }
            }
            let percent_string = line.get(prev_index..percent_index);
            if let Some(percent_string) = percent_string {
                if let Ok(percent_float) = percent_string.parse::<f64>() {
                    if ret_value.is_none() {
                        ret_value = Some(percent_float as u8);
                    }
                }
            }
        }
    }

    ret_value
}

pub async fn download_video(
    url: String,
    output_name: Option<String>,
    progress_holder: &'static Mutex<ProgressHolder<String>>,
) -> TaskResult {
    // form the command via all of the args it needs
    // and do basic spawn error checking
    let mut cmd = Command::new("youtube-dl");
    cmd.arg("--newline");
    cmd.arg("--ignore-config");
    cmd.arg(&url);
    if let Some(output_name) = output_name {
        cmd.arg("-o");
        cmd.arg(output_name);
    }
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            let error_string = format!("Failed to spawn child process: {}", e);
            return Err(error_string);
        }
    };
    let stdout = match child.stdout.take() {
        Some(s) => s,
        None => {
            let error_string = format!("Failed to get handle child process stdout");
            return Err(error_string);
        }
    };
    // create a reader from the stdout handle we created
    // pass that reader into the following future spawned on tokio
    let mut reader = BufReader::new(stdout).lines();
    tokio::spawn(async move {
        loop {
            let thing = reader.next_line().await;
            if let Err(_) = thing { break; }
            let thing = thing.unwrap();

            if let None = thing {
                break; // reach end of input, stop looping
            } else if let Some(ref line) = thing {
                let prog_opt = get_ytdl_progress(line);
                if let None = prog_opt { continue; }

                let progress = prog_opt.unwrap();
                use_me_from_progress_holder(&url, progress_holder, |me| {
                    println!("setting progress to {}", progress);
                    me.inc_progress_percent(progress as f64);
                });
            }
        }
    });

    // the above happens asynchronously, but here we await the child process
    // itself. as we await this child process, the above async future can run
    // whenever the reader finds a next line. But after here we actually return
    // our TaskResult that is read by the progresslib2
    let child_status = child.await;
    let status = match child_status {
        Ok(s) => s,
        Err(e) => {
            let error_string = format!("child process encountered an error: {}", e);
            return Err(error_string);
        }
    };

    match status.success() {
        true => Ok(None),
        false => {
            let error_code = status.code();
            if let None = error_code {
                return Err("child process failed to exit with a valid exit code".into());
            }
            let error_code = status.code().unwrap();
            let error_string = format!("child process exited with error code: {}", error_code);
            Err(error_string)
        }
    }
}


#[derive(Serialize, Deserialize)]
pub struct DownloadRequest {
    url: String,
    name: Option<String>,
}

async fn status() -> HttpResponse {
    let guard = PROGHOLDER.lock().unwrap();
    let mut output = String::from("ALL PROGRESSES:\n");
    for (key, progress) in guard.progresses.iter() {
        output.push_str(key);
        output.push_str(":\n  ");
        output.push_str(&progress.get_stage_name());
        output.push_str(": ");
        match progress.get_progress_error() {
            None => {
                let progress_percent = progress.get_progress_percent();
                output.push_str(&progress_percent.to_string());
                output.push_str("%");
            }
            Some(error_struct) => {
                output.push_str(&error_struct.error_string);
            }
        }
        output.push_str("\n");
    }
    drop(guard);
    HttpResponse::Ok().body(output).into()
}

async fn download(item: web::Json<DownloadRequest>) -> HttpResponse {
    let download_request = item.0;

    // we create our one and only stage named "download"
    // the second argument is a future. which we pass an async function
    // which returns a future. this future does not run anything until
    // we call progitem.start() below. note we can pass a reference to PROGHOLDER
    // only because it is static.
    let download_stage = make_stage!(download_video;
        download_request.url.clone(),
        download_request.name.clone(),
        &PROGHOLDER,
    );
    let mut progitem = ProgressItem::new();
    progitem.register_stage(download_stage);
    match PROGHOLDER.lock() {
        Err(_) => {
            HttpResponse::InternalServerError().body("Failed to acquire lock").into()
        }
        Ok(mut guard) => {
            // here we start the progress item, and immediately hand it off
            // to the progholder. note that the start method also takes the progholder
            // but because it is currently under a lock, if the progress item tries
            // to use the progholder it will fail. Thats why internally, the progress item
            // uses try_lock to avoid blocking, and it has retry capabilities.
            progitem.start(download_request.url.clone(), &PROGHOLDER);
            guard.progresses.insert(download_request.url, progitem);
            HttpResponse::Ok().into()
        }
    }
}

// in this example we use an actix server, but with
// the tokio runtime because we want to use
// tokio functions in our route handlers
#[tokio::main]
async fn main() -> std::io::Result<()> {
    let local = tokio::task::LocalSet::new();
    let sys = actix_web::rt::System::run_in_tokio("server", &local);
    let _ = HttpServer::new(|| {
        App::new()
            .route("/status", web::get().to(status))
            .route("/download", web::post().to(download))
    })
        .bind("0.0.0.0:4000")?
        .run()
        .await?;
    sys.await?;
    Ok(())
}
