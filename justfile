set dotenv-load := true

# Show available commands.
default:
    @just --list

# Install frontend dependencies and fetch Rust dependencies.
setup:
    npm run setup

# Run React and Rust together; Ctrl+C stops both.
dev:
    npm run dev

# Run the Rust API alone.
backend:
    cargo run --manifest-path backend/Cargo.toml --locked

# Run the React development server alone.
frontend:
    npm --prefix frontend run dev -- --host 127.0.0.1 --strictPort

# Format checks, Clippy, Rust tests, TypeScript checks, and frontend build.
check:
    npm run check

# Build the frontend and optimized Rust backend.
build:
    npm run build

# Serve the built frontend and API from Rust on port 8000.
start:
    npm start

# Run the preserved Python reference suite (requires requirements-dev.txt).
python-test:
    python -m pytest -q
