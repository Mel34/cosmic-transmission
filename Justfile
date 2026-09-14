# Check the project
check:
    cargo check

# Build the project
build:
    cargo build

# Run the applet
run:
    cargo run

# Check, then run
dev: check
    cargo run
