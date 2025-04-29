# archiver
Content curation, scraping, preservation and Codex integration

## Features
- Attempts to be a "good citizen" while scraping (respects robots.txt, etc.)
- Archive.org download (via the Archive.org APIs) - https://archive.org/developers/index.html

## Structure (for Archive.org downloads)
- Organises data first into collections, then into items as subfolders of those collections
- Also archives metadata about the collection and item

## Codex integration
- Uses the Codex APIs to upload content to Codex nodes, and keeps track of which CIDs have which content
- Keeps track of which collections and items have been uploaded to Codex
