# librarian
Librarian is a tool for archiving, curating and collecting web content, with a focus on the Riff platform. (https://librarian.riff.cc)

## Usage
- Install Rust
- Make sure you can write to /var/log/riffarchiver.log, then simply run `cargo run --release`.

## Planned Features
- Attempts to be a "good citizen" while scraping (respects robots.txt, etc.)
- Archive.org download (via the Archive.org APIs) - https://archive.org/developers/index.html
