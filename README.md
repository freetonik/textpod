[![textpod build status on GNU/Linux](https://github.com/freetonik/textpod/workflows/GNU%2FLinux/badge.svg)](https://github.com/freetonik/textpod/actions?query=workflow%3AGNU%2FLinux)
[![textpod build status on macOS](https://github.com/freetonik/textpod/workflows/macOS/badge.svg)](https://github.com/freetonik/textpod/actions?query=workflow%3AmacOS)
[![textpod build status on Windows](https://github.com/freetonik/textpod/workflows/Windows/badge.svg)](https://github.com/freetonik/textpod/actions?query=workflow%3AWindows)

# Textpod

Local, web-based note-taking app inspired by "One Big Text File" idea. Short demo:

[![Textpod short demo video](https://img.youtube.com/vi/VAqJJxaJNVM/0.jpg)](https://www.youtube.com/watch?v=VAqJJxaJNVM)

- Single page with all notes and a simple entry form (Markdown)
- All notes are stored in a single `notes.md` file
- Search/filtering when you start typing with `/`
- Start a link with `+` and Textpod will save a local single-page copy
- File and image attachments

## Installation

#### Using [Cargo](https://crates.io/crates/textpod) (cross-platform)

```console
cargo install textpod
```

#### Via [Homebrew](https://brew.sh/) (macOS and GNU/Linux)

```console
brew tap freetonik/tap
brew install textpod
```

In order to download webpages, you need to have `monolith` installed. `cargo install monolith` or `brew install monolith` (macOS). See [monolith](https://github.com/Y2Z/monolith) for more details.

## Usage

Run `textpod` in any directory. It will create a `notes.md` file if it doesn't exist. It will create `attachments` directory for file and image attachments.
Webpages are saved in `attachments/webpages`. You can specify the port with `-p` flag, e.g. `textpod -p 8080` and/or the address with `-l` flag, e.g. `textpod -l 0.0.0.0`.

## Docker

Docker image is available at [Docker Hub](https://hub.docker.com/r/freetonik/textpod).
E.g. run on port `8099`, mapping the `notes` directory (under current directory):

```
docker pull freetonik/textpod
docker run --rm --name textpod -d -v $(pwd)/notes:/app -p 8099:3000 freetonik/textpod
```

Or check out `docker-compose.yml`.

## Build and run

### Requirements
- Rust 1.80+ 

Build

```sh
cargo build
```

Run via binary (`target/debug/textpod`) or via cargo:

```sh
cargo run
```

## Contributing

Feel free to open issues and pull requests. I want to keep the code very simple and accessible to beginners. The goal is not to create another feature-rich note-taking app, but to keep it simple and fast.
A "one big text file" idea is very powerful and I just want to make it slightly enhanced.
