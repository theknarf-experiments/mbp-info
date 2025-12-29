# mbp-info

A terminal-based system monitoring application for MacBook Pro. It displays real-time CPU, GPU, memory, and thermal metrics in an interactive dashboard.

## Setup

```
cargo run
```

## Description

mbp-info provides a TUI (Terminal User Interface) with multiple panels showing system metrics, charts, and collector status. The interface supports vim-like commands (`:q` to quit, `:help` for available commands) and customizable key bindings with leader key support.

The application runs several data collectors in background threads, sampling metrics like CPU/GPU temperatures, memory usage, swap, and uptime. Some collectors (e.g., powermetrics for thermal data) require sudo privileges - the app will prompt for authentication on startup if needed.

### Command-line Options

- `--dump-sample [path]` - Run collectors once and output sample data to a file (default: `mbp-info-sample.txt`)
- `--collectors <list>` - Specify which collectors to run (comma-separated)
