# Librarian Project Instructions

## Important: Project Management

When starting work on this project, ALWAYS:

1. **First**, use the Plane MCP to look up the "Librarian" project (ID: 408c1523-be9b-4103-82d3-be2b250d9448, Identifier: LIBRA)
2. **Check current task status** in Plane to see what needs to be done
3. **Use Plane as the source of truth** for all tasks and subtasks
4. **Update task status in Plane** as you complete work

## Optional: Taskmaster Integration

If needed for complex task management:
- Use Taskmaster MCP to manage detailed implementation state
- But always defer to Plane for the official task list and status

## Project Overview

Librarian is an archiving and media player application for Riff.CC that:
- Archives content from Archive.org and other sources
- Provides a gamepad-first interface optimized for ultrawide monitors
- Supports both archiving and playback of media
- Built with Tauri + Vue.js + Pinia

## Key Features

1. **Tile-based browser** - Visual browsing of collections with visited/unvisited states
2. **Gamepad controls** - Full gamepad support with keyboard/mouse fallback
3. **Archive.org integration** - OAuth authentication and rate-limited API access
4. **qBittorrent integration** - Torrent downloading and management
5. **Smart import manager** - Quality detection and MusicBrainz metadata
6. **Media player** - Basic audio playback with plans for video
7. **Dark/light themes** - Ultrawide-optimized responsive design
8. **TOML configuration** - Settings stored in ~/.riffcc/librarian.toml

## Development Workflow

1. Check Plane for current tasks
2. Update task status to "in-progress" when starting
3. Implement the feature/fix
4. Test thoroughly
5. Update task status to "done" when complete
6. Move to the next task from Plane

## Important Paths

- Processing folder: `/mnt/mfs/riffcc/processing`
- Config location: `~/.riffcc/librarian.toml`