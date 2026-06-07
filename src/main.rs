use axum::{
    extract::{DefaultBodyLimit, Multipart, Path, State, Query},
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
use std::collections::HashMap;
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
    /// Optional positional mode token (use `readonly` to enable read-only mode)
    #[arg(value_name = "MODE")]
    mode: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct Note {
    timestamp: String,
    content: String,
    html: String,
    tags: Vec<String>,
}

#[derive(Clone)]
struct AppState {
    html: String,
    embed_html: String,
    notes: Arc<Mutex<Vec<Note>>>,
    notes_file: PathBuf,
    readonly: bool,
}

#[derive(Deserialize, Default)]
struct Config {
    #[serde(default)]
    shortcuts: ShortcutsConfig,
}

#[derive(Deserialize, Serialize)]
struct ShortcutsConfig {
    #[serde(default = "default_save_shortcut")]
    save: String,
}

impl Default for ShortcutsConfig {
    fn default() -> Self {
        Self {
            save: default_save_shortcut(),
        }
    }
}

fn default_save_shortcut() -> String {
    "Ctrl+Enter".to_string()
}

const CONTENT_LENGTH_LIMIT: usize = 500 * 1024 * 1024; // allow uploading up to 500mb files... overkill?

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    let args = Args::parse();

    // Use the positional token `readonly` to enable read-only mode, e.g.
    // `cargo run -- readonly`
    let readonly_flag = match args.mode.as_deref() {
        Some("readonly") => true,
        _ => false,
    };

    if let Some(path) = args.base_directory {
        if let Err(e) = env::set_current_dir(&path) {
            error!("could not change directory to {}: {e}", path.display());
            process::exit(1);
        }
    }

    let config: Config = match fs::read_to_string("textpod.toml") {
        Ok(contents) => match toml::from_str(&contents) {
            Ok(cfg) => cfg,
            Err(e) => {
                error!("failed to parse textpod.toml: {e}; using defaults");
                Config::default()
            }
        },
        Err(_) => Config::default(),
    };

    if let Err(e) = fs::create_dir_all("attachments") {
        error!(
            "could not create attachments directory in {}: {e}",
            env::current_dir().unwrap().display()
        );
        process::exit(1);
    }

    let favicon = Base64Display::new(FAVICON_SVG, &STANDARD);
    let shortcuts_json =
        serde_json::to_string(&config.shortcuts).expect("failed to serialize shortcuts config");
    let html = INDEX_HTML
        .replace(
            "{{FAVICON}}",
            format!("data:image/svg+xml;base64,{favicon}").as_str(),
        )
        .replace("{{SHORTCUTS_CONFIG}}", &shortcuts_json)
        .replace("{{SAVE_SHORTCUT_DISPLAY}}", &config.shortcuts.save);

    // Create an embed (read-only) version of the HTML by removing the editor
    // and the per-note delete link. This is used for iframe embedding and
    // also as the served root page when running in read-only mode.
    let mut embed_html = html.clone();
    // Remove the editor textarea and submit container
    if let Some(start) = embed_html.find("<textarea id=\"editor\"") {
        if let Some(end) = embed_html[start..].find("</textarea>") {
            // include the closing tag
            let end_idx = start + end + "</textarea>".len();
            // Also remove the submitContainer that follows
            if let Some(submit_pos) = embed_html[end_idx..].find("<div id=\"submitContainer\"") {
                if let Some(submit_end) = embed_html[end_idx + submit_pos..].find("</div>") {
                    let submit_end_idx = end_idx + submit_pos + submit_end + "</div>".len();
                    embed_html.replace_range(start..submit_end_idx, "");
                } else {
                    embed_html.replace_range(start..end_idx, "");
                }
            } else {
                embed_html.replace_range(start..end_idx, "");
            }
        }
    }

    // Remove the inline delete link rendered inside displayNotes()
    // The snippet in the template is: [<a href="#" onclick="deleteNote(${i})">delete</a>]
    embed_html = embed_html.replace("[<a href=\"#\" onclick=\"deleteNote(${i})\">delete</a>]", "");

    let notes = Arc::new(Mutex::new(load_notes(&args.notes_file)));

    // If running in read-only mode, set an env var so handlers without access to
    // State (like the multipart upload handler) can behave accordingly.
    if readonly_flag {
        std::env::set_var("TEXTPOD_READONLY", "1");
    }

    let state = AppState {
        html,
        embed_html: embed_html.clone(),
        notes,
        notes_file: args.notes_file,
        readonly: readonly_flag,
    };

    let app = Router::new()
        .route("/", get(index))
        .route("/embed", get(get_embed_with_query))
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

    if let Ok(listener) = tokio::net::TcpListener::bind(&addr).await {
        if let Err(e) = axum::serve(listener, app)
            .with_graceful_shutdown(shutdown())
            .await
        {
            error!("Server error: {}", e);
        }
    }
}

async fn shutdown() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("Failed to install ctrl + c handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("It's supposed to run in a unix system.")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
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
                let tags = extract_tags(&content);
                Note {
                    timestamp,
                    content: content.to_string(),
                    html,
                    tags,
                }
            })
            .collect()
    } else {
        Vec::new()
    }
}

// route / (root)
async fn index(State(state): State<AppState>) -> Html<String> {
    if state.readonly {
        Html(state.embed_html.clone())
    } else {
        Html(state.html.clone())
    }
}

// GET /embed?tag=... - return embeddable HTML filtered by tag
async fn get_embed_with_query(
    State(state): State<AppState>,
    Query(params): Query<HashMap<String, String>>,
) -> Html<String> {
    let tag = params.get("tag").map(|s| s.to_lowercase());

    let notes = state.notes.lock().unwrap();
    let filtered: Vec<Note> = notes
        .iter()
        .cloned()
        .filter(|n| match &tag {
            Some(t) => n.tags.iter().any(|tg| tg == t),
            None => true,
        })
        .collect();

    // Minimal HTML page for embedding filtered notes. Copy core styles from index.html.
    let mut out = String::new();
    out.push_str("<!DOCTYPE html><html><head><meta charset=\"utf-8\">\n");
    out.push_str("<meta name=\"color-scheme\" content=\"light dark\" />\n");
    out.push_str("<style>");
    out.push_str(".note{margin-bottom:1.75em;padding-top:0.25em}.note .noteMetadata{font-size:0.9em;font-family:monospace;color:#666}.note img,.note iframe,.note video,.note audio,.note embed,.note svg{max-width:100%}");
    out.push_str("</style></head><body>\n");

    out.push_str("<div id=\"notes\">\n");
    for note in filtered.iter().rev() {
        out.push_str("<div class=\"note\">\n");
        out.push_str(&note.html);
        out.push_str("<div class=\"noteMetadata\">\n");
        out.push_str(&format!("<time datetime=\"{}\">{}</time>", note.timestamp, note.timestamp));
        if !note.tags.is_empty() {
            out.push_str(" &nbsp; ");
            out.push_str(&note.tags.iter().map(|t| format!("#{}", t)).collect::<Vec<_>>().join(" "));
        }
        out.push_str("</div></div>\n");
    }
    out.push_str("</div></body></html>");

    Html(out)
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
    if state.readonly {
        return Err((StatusCode::METHOD_NOT_ALLOWED, String::from("read-only mode")));
    }
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
    if state.readonly {
        return Err(StatusCode::METHOD_NOT_ALLOWED);
    }
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
    let html = md_to_html(&content);
    let tags = extract_tags(&content);
    let note = Note {
        timestamp: timestamp.clone(),
        content: content.clone(),
        html,
        tags,
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
    // Check env var set at startup for readonly mode; this handler doesn't
    // currently receive State, so use env var as a pragmatic signal.
    if std::env::var("TEXTPOD_READONLY").is_ok() {
        return Err(StatusCode::METHOD_NOT_ALLOWED);
    }

    while let Some(field) = multipart.next_field().await.unwrap() {
        let name = field.file_name().unwrap().to_string();
        let data = field.bytes().await.unwrap();

        info!("Uploading file: {}", name);

        let original_path = PathBuf::from("attachments").join(&name);
        let mut counter = 1;

        let original_stem = original_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("");
        let original_ext = original_path
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("");

        // Generate unique filename if already exists
        let mut path = original_path.clone();
        while path.exists() {
            // e.g: file-1.txt
            let new_name = if original_ext.is_empty() {
                format!("{}-{}", original_stem, counter)
            } else {
                format!("{}-{}.{}", original_stem, counter, original_ext)
            };

            path = original_path.parent().unwrap().join(new_name);
            counter += 1;
        }

        fs::write(&path, data).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

        info!("File saved as {}", path.display());
        return Ok(Json(format!(
            "/attachments/{}",
            path.file_name().unwrap().to_str().unwrap()
        )));
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
    options.render.hardbreaks = true;
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

fn extract_tags(content: &str) -> Vec<String> {
    content
        .split_whitespace()
        .filter_map(|w| {
            if w.starts_with('#') && w.len() > 1 {
                // strip leading '#' and trailing punctuation
                let mut tag = w.trim_start_matches('#').trim().to_string();
                // remove surrounding punctuation
                tag = tag.trim_matches(|c: char| !c.is_alphanumeric() && c != '-' && c != '_').to_string();
                if !tag.is_empty() {
                    return Some(tag.to_lowercase());
                }
            }
            None
        })
        .collect()
}
