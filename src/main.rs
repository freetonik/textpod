use axum::{
    extract::{DefaultBodyLimit, Multipart, Path, State},
    http::StatusCode,
    response::{Html, IntoResponse},
    routing::{get, post},
    Json, Router,
};
use base64::{display::Base64Display, engine::general_purpose::STANDARD};
use chrono::Local;
use clap::Parser;
use comrak::{markdown_to_html, Options};
use serde::{Deserialize, Serialize};
use std::{
    env,
    fs::{self},
    io::Write,
    net::SocketAddr,
    path::PathBuf,
    process,
    sync::{Arc, Mutex},
};
use tokio::process::Command;
use tokio::spawn;
use tower_http::services::ServeDir;
use tracing::{error, info};
use tracing_subscriber;

const INDEX_HTML: &str = include_str!("index.html");
const FAVICON_SVG: &[u8] = include_bytes!("favicon.svg");

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Change to DIR before doing anything
    #[arg(short = 'C', long, value_name = "DIR")]
    base_directory: Option<PathBuf>,
    /// Port number for the server
    #[arg(short, long, default_value_t = 3000)]
    port: u16,
    /// Listen address for the server
    #[arg(short, long, default_value = "127.0.0.1")]
    listen: String,
    /// Save notes in FILE
    #[arg(short = 'f', long, value_name = "FILE", default_value = "notes.md")]
    notes_file: PathBuf,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct Note {
    timestamp: String,
    content: String,
    html: String,
}

#[derive(Clone)]
struct AppState {
    html: String,
    notes: Arc<Mutex<Vec<Note>>>,
    notes_file: PathBuf,
}

const CONTENT_LENGTH_LIMIT: usize = 500 * 1024 * 1024; // allow uploading up to 500mb files... overkill?

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    let args = Args::parse();

    if let Some(path) = args.base_directory {
        if let Err(e) = env::set_current_dir(&path) {
            error!("could not change directory to {}: {e}", path.display());
            process::exit(1);
        }
    }

    if let Err(e) = fs::create_dir_all("attachments") {
        error!(
            "could not create attachments directory in {}: {e}",
            env::current_dir().unwrap().display()
        );
        process::exit(1);
    }

    let favicon = Base64Display::new(FAVICON_SVG, &STANDARD);
    let html = INDEX_HTML.replace(
        "{{FAVICON}}",
        format!("data:image/svg+xml;base64,{favicon}").as_str(),
    );

    let notes = Arc::new(Mutex::new(load_notes(&args.notes_file)));

    let state = AppState {
        html,
        notes,
        notes_file: args.notes_file,
    };

    let app = Router::new()
        .route("/", get(index))
        .route("/notes", get(get_notes).post(save_note))
        .route(
            "/notes/:index",
            get(get_note_by_index).delete(delete_note_by_index),
        ) // TODO PUT/PATCH
        .route("/upload", post(upload_file))
        .layer(DefaultBodyLimit::max(CONTENT_LENGTH_LIMIT))
        .nest_service("/attachments", ServeDir::new("attachments"))
        .with_state(state);

    let server_details = format!("{}:{}", args.listen, args.port);
    let addr: SocketAddr = server_details
        .parse()
        .expect("Unable to parse socket address");
    info!("Starting server on http://{}", addr);

    match tokio::net::TcpListener::bind(&addr).await {
        Ok(listener) => {
            if let Err(e) = axum::serve(listener, app).await {
                error!("Server error: {}", e);
            }
        }
        Err(e) => {
            error!("Failed to bind to address {}: {}", addr, e);
        }
    }
}

fn load_notes(file: &PathBuf) -> Vec<Note> {
    if let Ok(content) = fs::read_to_string(file) {
        content
            .split("\n\n---\n\n")
            .filter(|s| !s.trim().is_empty())
            .map(|block| {
                let parts: Vec<&str> = block.splitn(2, '\n').collect();
                let (timestamp, content) = match parts.as_slice() {
                    [timestamp, content] => {
                        (timestamp.trim().to_string(), content.trim().to_string())
                    }
                    _ => (
                        Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
                        block.to_string(),
                    ),
                };

                let html = md_to_html(&content);
                Note {
                    timestamp,
                    content: content.to_string(),
                    html,
                }
            })
            .collect()
    } else {
        Vec::new()
    }
}

// route / (root)
async fn index(State(state): State<AppState>) -> Html<String> {
    Html(state.html)
}

// GET /notes
async fn get_notes(State(state): State<AppState>) -> Json<Vec<Note>> {
    let notes = state.notes.lock().unwrap();
    Json(notes.iter().cloned().collect::<Vec<_>>())
}

// GET /notes/:index
async fn get_note_by_index(
    State(state): State<AppState>,
    Path(index): Path<usize>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let notes = state.notes.lock().unwrap();
    if index >= notes.len() {
        return Err((
            StatusCode::BAD_REQUEST,
            format!("request for non-existent note #{index}"),
        ));
    }

    return Ok(Json(notes.iter().collect::<Vec<_>>()[index].clone()));
}

// DELETE /notes/:index
async fn delete_note_by_index(
    State(state): State<AppState>,
    Path(index): Path<usize>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let mut notes = state.notes.lock().unwrap();
    if index >= notes.len() {
        return Err((
            StatusCode::BAD_REQUEST,
            format!("request for non-existent note #{index}"),
        ));
    }

    notes.remove(index);

    // Update the notes file
    let content = notes
        .iter()
        .map(|note| format!("{}\n{}\n\n---\n\n", note.timestamp, note.content))
        .collect::<String>();

    if let Err(e) = fs::write(&state.notes_file, content) {
        return Err((StatusCode::INTERNAL_SERVER_ERROR, e.to_string()));
    }

    info!("Note deleted: {}", index);

    // TODO return the deleted note, maybe?
    return Ok(StatusCode::NO_CONTENT);
}

// POST /notes
async fn save_note(
    State(state): State<AppState>,
    Json(content): Json<String>,
) -> Result<(), StatusCode> {
    let mut content = content.clone();

    // Replace "---" with "<hr>" in the content
    content = content.replace("---", "<hr>");
    let links_to_download: Vec<String> = content
        .split_whitespace()
        .filter(|word| word.starts_with("+http"))
        .map(|s| s.to_string())
        .collect();

    fs::create_dir_all("attachments/webpages").unwrap();

    for link in &links_to_download {
        let url = &link[1..];
        let escaped_filename = url_to_safe_filename(url);
        let filepath = format!("attachments/webpages/{}.html", escaped_filename);
        content = content.replace(link, &format!("{} ([local copy](/{}))", url, filepath));
    }

    let timestamp = Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    let html = md_to_html(&content); // Changed to pass a reference
    let note = Note {
        timestamp: timestamp.clone(),
        content: content.clone(),
        html,
    };

    state.notes.lock().unwrap().push(note);

    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&state.notes_file)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    write!(file, "{}\n{}\n\n---\n\n", timestamp, content)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    info!("Note created: {}", timestamp);

    if !links_to_download.is_empty() {
        let notes = state.notes.clone();
        spawn(async move {
            for link in links_to_download {
                let url = &link[1..];
                let escaped_filename = url_to_safe_filename(url);
                let filepath = format!("attachments/webpages/{}.html", escaped_filename);

                let result = Command::new("monolith")
                    .args(&[url, "-o", &filepath])
                    .output()
                    .await;

                info!("Downloading webpage: {}", url);

                if result.is_err() {
                    error!("Failed to download webpage: {}", url);
                    let mut notes_lock = notes.lock().unwrap();
                    if let Some(last_note) = notes_lock.last_mut() {
                        let updated_content = last_note.content.replace(
                            &format!("([local copy](/{}))", filepath),
                            "(local copy failed)",
                        );
                        last_note.content = updated_content.clone();
                        last_note.html = md_to_html(&updated_content); // Changed to pass a reference here too

                        drop(notes_lock);

                        if let Ok(file_content) = fs::read_to_string(&state.notes_file) {
                            let notes_lock = notes.lock().unwrap();
                            let updated_content: Vec<String> = file_content
                                .split("\n---\n")
                                .enumerate()
                                .map(|(i, note_content)| {
                                    if i == notes_lock.len() - 1 {
                                        format!("{}\n{}", timestamp, updated_content)
                                    } else {
                                        note_content.to_string()
                                    }
                                })
                                .collect();
                            drop(notes_lock);

                            if let Ok(mut file) = fs::File::create(&state.notes_file) {
                                for note_content in updated_content {
                                    writeln!(file, "{}\n---", note_content).ok();
                                }
                            }
                        }
                    }
                }
            }
        });
    }

    Ok(())
}

// route POST /upload
async fn upload_file(mut multipart: Multipart) -> Result<Json<String>, StatusCode> {
    while let Some(field) = multipart.next_field().await.unwrap() {
        let name = field.file_name().unwrap().to_string();
        let data = field.bytes().await.unwrap();

        info!("Uploading file: {}", name);
        let path = PathBuf::from("attachments").join(&name);
        fs::write(path, data).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

        return Ok(Json(format!("/attachments/{}", name)));
    }

    error!("Error uploading file");
    Err(StatusCode::BAD_REQUEST)
}

// UTILS
fn md_to_html(markdown: &str) -> String {
    let mut options = Options::default();
    options.extension.strikethrough = true;
    options.extension.tagfilter = true;
    options.extension.table = true;
    options.extension.autolink = true;
    options.extension.tasklist = true;
    options.extension.superscript = true;
    options.render.unsafe_ = true;
    markdown_to_html(markdown, &options)
}

fn url_to_safe_filename(url: &str) -> String {
    let mut safe_name = String::with_capacity(url.len());

    let stripped_url = url
        .trim()
        .strip_prefix("http://")
        .unwrap_or(url)
        .strip_prefix("https://")
        .unwrap_or(url);

    for c in stripped_url.chars() {
        match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => safe_name.push('_'),
            c if c.is_alphanumeric() || c == '-' || c == '.' || c == '_' => safe_name.push(c),
            _ => safe_name.push('_'),
        }
    }

    safe_name.trim_matches(|c| c == '.' || c == ' ').to_string()
}
