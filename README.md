# Rust Call Graph Generator

A tool for generating and analyzing call graphs for Rust projects.

## Overview

This tool allows you to generate a call graph for your Rust project, which can help with:
- Understanding code flow
- Detecting bugs
- Analyzing dependencies between functions

## Installation

Install `cg`, `cargo-cg`, and `call-cg` by running:

```bash
cargo install --path .
```

## Usage

### Basic Usage

Navigate to your project directory and run:

```bash
call-cg
```

This will generate a call graph and save it to `./target/callgraph.txt`.

### Deduplication

By default, the deduplication feature is enabled. This feature removes duplicate callees and keeps only the shortest path.

To disable deduplication and show all call paths:

```bash
call-cg --no-dedup
```

### Options

To see all available options:

```bash
call-cg -h
```

Output:
```
This is a bug detector for Rust.

Usage: cg [OPTIONS] [CARGO_ARGS]...

Arguments:
  [CARGO_ARGS]...  Arguments passed to cargo rust-analyzer

Options:
      --show-all-funcs             Show all functions
      --show-all-mir               Show all MIR
      --emit-mir                   Emit MIR
      --entry-point <ENTRY_POINT>  Entry point of the program
  -o, --output-dir <OUTPUT_DIR>    Output directory
      --no-dedup                   No deduplication for call sites When enabled, keeps all call sites for the same caller-callee pair
  -h, --help                       Print help
```

## Examples

### Analyzing a Test Directory

```bash
cd test_directory
call-cg
# Call graph will be available at ./target/callgraph.txt
```

### Using Custom Output Directory

```bash
call-cg -o ./custom_output
# Call graph will be available at ./custom_output/callgraph.txt
```

## License

This project is dual-licensed under both:

- [Apache License 2.0](https://www.apache.org/licenses/LICENSE-2.0)
- [MIT License](https://opensource.org/licenses/MIT)

You may choose either license at your option.



