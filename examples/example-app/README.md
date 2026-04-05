This directory contains the checked-in inputs and reference output for `cargo run --example example_app`.

Layout:

- `system/` simulates level 2 discovered configuration
- `install-root/etc/` simulates level 3 installation-relative configuration
- `home/.config/example-app/` simulates level 4 user configuration
- `cwd/` simulates level 5 working-directory configuration and the level 7 CLI config file
- `output.txt` is the expected example output
