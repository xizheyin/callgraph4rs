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
      --no-dedup                   Disable deduplication of call sites for the same caller-callee pair
                                   When enabled, keeps all call sites
      --find-callers-of <FUNCTION_PATH> Find all functions that directly or indirectly call the specified function
      --json-output                Output the call graph in JSON format
      --without-args               Do not include generic type arguments in function paths
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

### Outputting Call Graph in JSON Format

For machine-readable output that can be processed by other tools:

```bash
call-cg --json-output
# JSON output will be available at ./target/callgraph.json
```

The JSON format includes detailed information about each caller and its callees, including:
- Function names
- Paths
- Version information (extracted from crate metadata)
- Call constraint depths

Example JSON structure:
```json
[
  {
    "caller": {
      "name": "example_caller_name",
      "version": "1.0.0",
      "path": "example/path/to/caller",
      "constraint_depth": 3
    },
    "callee": [
      {
        "name": "example_callee_name_1",
        "version": "1.1.0",
        "path": "example/path/to/callee_1",
        "constraint_depth": 2
      },
      {
        "name": "example_callee_name_2",
        "version": "1.2.0",
        "path": "example/path/to/callee_2",
        "constraint_depth": 1
      }
    ]
  }
]
```

This format is ideal for further processing or visualization with external tools.

### Finding All Callers of a Function

To find all functions that directly or indirectly call a specific function:

```bash
call-cg --find-callers-of "std::collections::HashMap::insert"
```

This will generate a report of all callers in `./target/callers.txt`.

You can use a partial path - the tool will match any function containing that substring. The matching behavior works as follows:

- If the path includes generic parameters (contains `<` character), it will match against the full function path including generic parameters or the basic path
- If the path does not include generic parameters, the tool will intelligently remove all generic parameter sections (`::<...>`) from function paths before matching, providing clean results when searching for generic functions

Examples:
```bash
# Match any DataStore::total_value regardless of generic parameters
# This will remove all generic parts from paths when matching
call-cg --find-callers-of "DataStore::total_value"

# Find callers of other crates
call-cg --find-callers-of "std::collections::HashMap::new"

# Match only functions that contain this specific generic instantiation in their path
# Using precise generic parameter syntax to match specific instances
RUST_LOG=off call-cg --find-callers-of "DataStore::<Electronics>::total_value"

# Find all callers of HashMap::new method from standard library, using full generic path
RUST_LOG=off call-cg --find-callers-of "std::collections::HashMap::<K, V>::new"
```

## Testing

This repository includes a test project (`test_callgraph`) designed to test the call graph generation capabilities. It contains a sample Rust program with complex call relationships involving traits, generics, closures, and more.

### Running the Test Project

1. First, make sure you have installed the tool:
   ```bash
   cargo install --path .
   ```

2. Navigate to the test project directory:
   ```bash
   cd test_callgraph
   ```

3. Build the test project:
   ```bash
   cargo build
   ```

4. Run the call graph analyzer:
   ```bash
   call-cg
   ```
   This will generate a call graph at `./target/callgraph.txt`.

5. Try finding callers of specific functions, for example:
   ```bash
   # Find all callers of Product::discounted_price
   call-cg --find-callers-of "Product::discounted_price"
   
   # Find all callers of DataStore::calculate_value_with_strategy
   call-cg --find-callers-of "DataStore::calculate_value_with_strategy"
   ```

6. To see all possible call paths without deduplication:
   ```bash
   call-cg --no-dedup
   ```

For detailed information about the test project structure and specific test cases, please refer to the documentation in the `test_callgraph` directory.

## License

This project is dual-licensed under both:

- [Apache License 2.0](https://www.apache.org/licenses/LICENSE-2.0)
- [MIT License](https://opensource.org/licenses/MIT)

You may choose either license at your option.



