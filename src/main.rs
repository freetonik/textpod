use axum::{
    extract::{DefaultBodyLimit, Multipart},
    extract::{Path, State},
    http::StatusCode,
    response::Html,
    routing::{get, post},
    Json, Router,
};
use chrono::Local;
use clap::Parser;
use comrak::{markdown_to_html, Options};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self},
    io::Write,
    net::SocketAddr,
    path::PathBuf,
    sync::{Arc, Mutex},
};
use tokio::process::Command;
use tokio::spawn;
use tower_http::services::ServeDir;
use tracing::{info, error};
use tracing_subscriber;

const INDEX_HTML: &str = r#"<!DOCTYPE html>
<html>
<head>
    <title>Textpod</title>
    <meta name="color-scheme" content="light dark"/>
    <link rel="shortcut icon" href="data:image/svg+xml;base64,PHN2ZyB3aWR0aD0iMjQiIGhlaWdodD0iMjQiIGZpbGw9ImN1cnJlbnRDb2xvciIgdmlld0JveD0iMCAwIDI0IDI0IiB4bWxucz0iaHR0cDovL3d3dy53My5vcmcvMjAwMC9zdmciPjxwYXRoIGQ9J00xMS42NjkgMi4yODJjLjIxOC0uMDQzLjQ0My0uMDQzLjY2MiAwIC4yNTEuMDQ4LjQ3OS4xNjcuNjkxLjI3N2wuMDUzLjAyOCA4LjI3IDQuMjhhLjc1Ljc1IDAgMCAxIC40MDUuNjY2djcuODk3YzAgLjI4My4wMDIuNTgzLS4wOTMuODYyYTEuNzU4IDEuNzU4IDAgMCAxLS4zOTUuNjUyYy0uMjA1LjIxNC0uNDczLjM1MS0uNzIzLjQ4bC0uMDYzLjAzMy04LjEzMSA0LjIwOGEuNzUuNzUgMCAwIDEtLjY5IDBsLTguMTMxLTQuMjA4LS4wNjMtLjAzM2MtLjI1LS4xMjktLjUxOC0uMjY2LS43MjMtLjQ4YTEuNzU5IDEuNzU5IDAgMCAxLS4zOTUtLjY1MmMtLjA5NS0uMjgtLjA5NC0uNTgtLjA5My0uODYzVjcuNTMzYS43NS43NSAwIDAgMSAuNDA1LS42NjZsOC4yNjktNC4yOC4wNTMtLjAyN2MuMjEzLS4xMTEuNDQtLjIzLjY5Mi0uMjc4bS4yMjYgMS40OTZhNi41NzkgNi41NzkgMCAwIDAtLjI4Mi4xNDFMNC42NjggNy41MTQgMTIgMTEuMTAybDcuMzMyLTMuNTg4LTYuOTQ2LTMuNTk1YTYuNTA1IDYuNTA1IDAgMCAwLS4yODItLjE0MS40OC40OCAwIDAgMC0uMDU4LS4wMjRtLS43OTYgMTYuMDEzdi03LjM2MmwtNy41LTMuNjd2Ni42MjRjMCAuMTg3IDAgLjI5NC4wMDUuMzc1YS40OTYuNDk2IDAgMCAwIC4wMDkuMDc4LjI1OC4yNTggMCAwIDAgLjA1Ny4wOTVjLjAwNS4wMDQuMDIxLjAxNy4wNjQuMDQyLjA2OC4wNDIuMTYzLjA5LjMyOC4xNzZ6bS42NDUtMTUuOTlhLjQ4My40ODMgMCAwIDEgLjA2LS4wMjN6Jy8+PC9zdmc+" />
    <style>
        @media (prefers-color-scheme: light) {
            time {
                color: #666;
            }
            .note code {
                background-color: #dedcd1;
            }
            .note pre {
                background-color: #dedcd1;
            }
        }
        @media (prefers-color-scheme: dark) {
            time {
                color: #BBB;
            }
            .note code {
                background-color: #3a3a3a;
            }
            .note pre {
                background-color: #3a3a3a;
            }
        }
        html {
            height: 100%;
        }
        body {
            font-family: system-ui, -apple-system, sans-serif;
            max-width: 800px;
            margin: 0 auto;
            padding: 20px;
        }
        #editor {
            width: 100%;
            height: 200px;
            margin-bottom: 2em;
            font-family: monospace;
            padding: 1em;
            resize: vertical;
            box-sizing: border-box
        }
        .note > :first-child {
            margin-top: 0;
        }
        .note > :nth-last-child(2) {
            margin-bottom: 0.5em;
        }
        .note {
            margin-bottom: 1.75em;
            padding-top: 0.25em;
        }
        .note code {
            padding: 0.25em;
        }
        .note pre {
            padding: 0.5em;
        }
        .note pre code {
            padding: 0;
            background-color: transparent;
        }
        .note img, .note iframe, .note video, .note audio, .note embed, .note svg {
            max-width: 100%;
        }
        time {
            font-size: 0.9em;
            margin-bottom: 0.25em;
            font-family: monospace;
        }
    </style>
</head>
<body>

    <textarea id="editor" placeholder="Ctrl+Enter to save.&#10;Type / to search.&#10;Drag & drop files to attach.&#10;Start links with + to save local copies."></textarea>
    <div id="notes">{{NOTES}}</div>

    <script>
        const editor = document.getElementById('editor');
        const notesDiv = document.getElementById('notes');
        let searchTimeout = null;
        let originalNotes = notesDiv.innerHTML;

        // Check for search parameter on page load
        window.addEventListener('load', () => {
            const params = new URLSearchParams(window.location.search);
            const searchQuery = params.get('q');
            if (searchQuery) {
                editor.value = '/' + decodeURIComponent(searchQuery);
                performSearch(searchQuery);
            }
        });

        async function performSearch(query) {
            const response = await fetch(`/search/${encodeURIComponent(query)}`);
            if (response.ok) {
                const notes = await response.json();
                displayNotes(notes);
            }
        }

        editor.addEventListener('input', async (e) => {
            const text = editor.value;
            if (text.startsWith('/')) {
                if (searchTimeout) {
                    clearTimeout(searchTimeout);
                }
                searchTimeout = setTimeout(async () => {
                    const query = text.slice(1);
                    // Update URL with search parameter
                    const newUrl = query
                        ? `${window.location.pathname}?q=${encodeURIComponent(query)}`
                        : window.location.pathname;
                    window.history.replaceState({}, '', newUrl);

                    if (query) {
                        await performSearch(query);
                    }
                }, 100);
            } else if (text === '') {
                // Clear search parameter from URL
                window.history.replaceState({}, '', window.location.pathname);
                notesDiv.innerHTML = originalNotes;
            }
        });

        editor.addEventListener('keydown', async (e) => {
            if (e.ctrlKey && e.key === 'Enter' && !editor.value.startsWith('/')) {
                const response = await fetch('/save', {
                    method: 'POST',
                    headers: {'Content-Type': 'application/json'},
                    body: JSON.stringify(editor.value)
                });

                if (response.ok) {
                    editor.value = '';
                    // Clear search parameter from URL when saving
                    window.history.replaceState({}, '', window.location.pathname);

                    const notesResponse = await fetch('/');
                    if (notesResponse.ok) {
                        const text = await notesResponse.text();
                        const tempDiv = document.createElement('div');
                        tempDiv.innerHTML = text;
                        const newNotes = tempDiv.querySelector('#notes').innerHTML;
                        notesDiv.innerHTML = newNotes;
                        originalNotes = newNotes;
                    }
                }
            }
        });

        editor.addEventListener('dragover', (e) => {
            e.preventDefault();
        });

        editor.addEventListener('drop', async (e) => {
            e.preventDefault();

            const files = e.dataTransfer.files;
            for (const file of files) {
                const formData = new FormData();
                formData.append('file', file);

                const response = await fetch('/upload', {
                    method: 'POST',
                    body: formData
                });

                if (response.ok) {
                    const path = await response.json();
                    const filename = path.split('/').pop();

                    const position = editor.selectionStart;
                    const before = editor.value.substring(0, position);
                    const after = editor.value.substring(position);

                    // Link path with spaces needs to be wrapped in < >, Commonmark spec apparently handles this
                    const needsBrackets = path.includes(' ') || filename.includes(' ');
                    const formattedPath = needsBrackets ? `<${path}>` : path;

                    if (file.type.startsWith('image/')) {
                        editor.value = `${before}![${filename}](${formattedPath})${after}`;
                    } else {
                        editor.value = `${before}[${filename}](${formattedPath})${after}`;
                    }
                }
            }
        });

        function displayNotes(notes) {
            notesDiv.innerHTML = notes
                .map(note => `
                    <div class="note">
                        ${note.html}
                        <time datetime="${note.timestamp}">${note.timestamp}</time>
                    </div>`)
                .join('');
        }
    </script>
</body>
</html>"#;

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Port number for the server
    #[arg(short, long, default_value_t = 3000)]
    port: u16,
    /// Listen address for the server
    #[arg(short, long, default_value_t = String::from("127.0.0.1"))]
    listen: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct Note {
    timestamp: String,
    content: String,
    html: String,
}

#[derive(Clone)]
struct AppState {
    notes: Arc<Mutex<Vec<Note>>>,
}

const CONTENT_LENGTH_LIMIT: usize = 500 * 1024 * 1024; // allow uploading up to 500mb files... overkill?

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    let args = Args::parse();
    fs::create_dir_all("attachments").unwrap();

    let state = AppState {
        notes: Arc::new(Mutex::new(load_notes())),
    };

    let app = Router::new()
        .route("/", get(index))
        .route("/save", post(save_note))
        .route("/search/:query", get(search_notes))
        .route("/upload", post(upload_file))
        .layer(DefaultBodyLimit::max(CONTENT_LENGTH_LIMIT))
        .nest_service("/attachments", ServeDir::new("attachments"))
        .with_state(state);

    let server_details = format!("{}:{}", args.listen, args.port);
    let addr: SocketAddr = server_details.parse().expect("Unable to parse socket address");
    println!("Starting server on http://{}", addr);

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

fn load_notes() -> Vec<Note> {
    if let Ok(content) = fs::read_to_string("notes.md") {
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
    let notes = state.notes.lock().unwrap();
    let notes_html = notes
        .iter()
        .rev()
        .map(|note| {
            format!(
                "<div class=\"note\">{}<time datetime=\"{}\">{}</time></div>",
                note.html, note.timestamp, note.timestamp
            )
        })
        .collect::<Vec<_>>()
        .join("\n");

    let html = INDEX_HTML.replace("{{NOTES}}", &notes_html);
    Html(html)
}

// route /save
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
        .open("notes.md")
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    writeln!(file, "{}\n{}\n\n---\n", timestamp, content)
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

                        if let Ok(file_content) = fs::read_to_string("notes.md") {
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

                            if let Ok(mut file) = fs::File::create("notes.md") {
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

// route GET /search/{query}
async fn search_notes(State(state): State<AppState>, Path(query): Path<String>) -> Json<Vec<Note>> {
    let notes = state.notes.lock().unwrap();
    let filtered: Vec<Note> = notes
        .iter()
        .filter(|note| note.content.to_lowercase().contains(&query.to_lowercase()))
        .cloned()
        .collect();
    Json(filtered)
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
