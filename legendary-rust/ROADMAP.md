# Roadmap: Missing Features from Original Legendary (Python)

This document tracks features present in the original Python implementation of Legendary that are currently missing or incomplete in this Rust reimplementation.

## Core Features
- [ ] **Aliases**: Support for defining and using app name aliases.
- [ ] **EULA Management**: Capability to view and accept EULAs required by some games.
- [ ] **Move Game**: Interactive or command-line utility to move installed game folders and update metadata.
- [ ] **Cleanup**: Tooling to remove outdated manifests, temporary files, and orphaned metadata.
- [ ] **Info View**: Detailed metadata and manifest information display (equivalent to `legendary info`).

## Authentication
- [ ] **EGL Auth Import**: Import existing login sessions from an installed Epic Games Launcher.
- [ ] **WebView Login**: Integrated WebView for a more streamlined login experience (currently SID/Code only).

## Third-Party Integration
- [ ] **Partner Activation**: Activate games on Ubisoft Connect and Origin/EA App.
- [ ] **EGL Export**: Export legendary-managed game installations back to the Epic Games Launcher.
- [ ] **Enhanced CrossOver Support**: Interactive bottle setup and configuration for macOS users.

## Downloader & Installation
- [ ] **Advanced Download Options**: IP binding, preferred CDN selection, and fine-grained delta manifest control.
- [ ] **Update Management**: Global update check for all installed games.
- [ ] **Integrity Checks**: Background or scheduled verification of installed games.

## Data Export & CLI
- [ ] **CLI Interface**: Complement the GUI with a full-featured command-line interface.
- [ ] **Export Formats**: Support for CSV, TSV, and JSON output for game lists and manifests.
- [ ] **Advanced Filtering**: Support for filtering game lists by category (e.g., Unreal Engine content, DLC, third-party apps).
