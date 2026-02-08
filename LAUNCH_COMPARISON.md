# Comparison: Legendary (Python) vs. Legendary-Rust Launch Logic

This document compares the game launch implementation in the original Python version of Legendary and the new Rust-based reimplementation.

## 1. Relevant Files

### Python (`legendary`)
*   `legendary/core.py`: Contains the core logic for constructing launch commands, resolving executables, and preparing the environment.
*   `legendary/cli.py`: The entry point for the `launch` command, handling CLI arguments and user interactions.
*   `legendary/models/game.py`: Defines the `LaunchParameters` structure used to pass data between the CLI and core logic.
*   `legendary/api/egs.py`: Handles API requests to Epic Games Services, such as obtaining exchange codes and ownership tokens.
*   `legendary/lfs/crossover.py`: Provides helper functions for detecting and configuring CrossOver on macOS.
*   `legendary/utils/env.py`: Manages platform-specific environment variables and detection.
*   `legendary/utils/egl_crypt.py`: Implements AES decryption used for importing and reading encrypted Epic Games Launcher user data/sessions.

### Rust (`legendary-rust`)
*   `legendary-rust/src/app.rs`: The central background worker that processes `LaunchGame` messages and executes the actual system process.
*   `legendary-rust/src/api.rs`: The `EgsClient` implementation for fetching authentication tokens and ownership validation files.
*   `legendary-rust/src/config.rs`: Handles loading and merging global and game-specific launch configurations (e.g., start parameters, compatibility tools).
*   `legendary-rust/src/eos.rs`: Manages detection and registry integration for the EOS Overlay.
*   `legendary-rust/src/models.rs`: Contains the data models for local metadata and installed game information used during launch.

## 2. Launch Process Overview

Both implementations follow a similar high-level process to launch a game:
1.  **Resolve Executable**: Determine the path to the game binary.
2.  **Authenticate**: Obtain a fresh exchange code (OAuth token) from Epic Games Services.
3.  **DRM/Ownership**: Fetch an Ownership Token (.ovt) if required by the game.
4.  **Construct Command**: Build the command line with standard Epic Games Launcher (EGL) arguments and environment variables.
5.  **Execution**: Launch the process, optionally using compatibility tools (Wine, Proton, etc.).

## 3. Key Technical Differences

### Executable Detection
*   **Python (`legendary`)**: Primarily relies on the `executable` path provided in the game metadata/manifest. Allows a manual override via the `override_exe` configuration option.
*   **Rust (`legendary-rust`)**: Implements a more robust multi-stage search:
    1.  Uses `custom_exe_path` if specified in settings.
    2.  Uses the path from the manifest (`installed.executable`).
    3.  Searches for files matching the `app_name` or `FolderName` attribute with common extensions (`.exe`, `.sh`).
    4.  As a fallback, scans the entire installation directory for `.exe` files, excluding known non-game binaries (e.g., uninstallers, redistributables).

### Compatibility & Linux Support
*   **Python**: Flexible but manual. Users specify a `wrapper` command or `wine_executable`. It has specialized logic for CrossOver on macOS.
*   **Rust**: Provides first-class integration for modern Linux gaming tools:
    *   **UMU Launcher**: Native support for the Unified Mukako Universal launcher.
    *   **Proton/Wine**: Structured selection of Steam Proton, Custom Proton/Wine, or System Wine via a dedicated `CompatibilityTool` enum.
    *   **Steam Integration**: Automatically handles Steam Compatibility environment variables (`STEAM_COMPAT_DATA_PATH`, etc.).

### EOS Overlay
*   **Python**: Manages the EOS Overlay primarily through Windows registry entries or the `EOS_OVERLAY_KILLED` environment variable.
*   **Rust**: Integrates EOS Overlay management directly into the background worker. It can automatically install/update the overlay and uses the `EOS_OVERLAY_KILLED` variable to enable/disable it based on user settings or presence.

## 4. Feature Gaps in Rust

While the Rust implementation offers a modern GUI and streamlined launch flow, several features from the original Python version are still missing:

| Feature | Python (`legendary`) | Rust (`legendary-rust`) |
| :--- | :--- | :--- |
| **EA/Origin Support** | Supported via `link2ea://` URIs | Missing |
| **Ubisoft Support** | Activation & Uplay requirement checks | Missing |
| **Aliases** | Supported (`legendary alias ...`) | Missing |
| **EGL Auth Import** | Can import session from EGL | Missing |
| **WebView Login** | Optional integrated login | SID/Code only |
| **CLI Interface** | Full-featured CLI | GUI-only |
| **EULA Management**| View/Accept required EULAs | Missing |
| **Move Game** | `legendary move ...` | Missing |
| **Detailed Info** | `legendary info ...` | Basic view only |
| **Export Formats** | CSV/JSON/TSV output | Missing |

## 5. Conclusion

The Rust reimplementation (`legendary-rust`) provides a more "intelligent" launch experience, particularly for Linux users, through its advanced executable searching and native UMU/Proton integration. However, the original Python version remains more feature-complete regarding third-party store integrations (EA, Ubisoft) and advanced management utilities (aliases, game moving, EGL session import).
