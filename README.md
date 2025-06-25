# Rust Call Graph Generator

A tool for generating and analyzing call graphs for Rust projects.

## Overview

This tool allows you to generate a call graph for your Rust project, which can help with:
- Understanding code flow
- Detecting bugs
- Analyzing dependencies between functions

## Installation

Install `cg`, `cargo-cg`, and `call-cg4rs` by running:

```bash
cargo install --path .
```

## Usage

### Basic Usage

Navigate to your project directory and run:

```bash
call-cg4rs
```

This will generate a call graph and save it to `./target/callgraph.txt`.
Default `cargo clean`, you can pass `--no-clean` to off the step.

### Deduplication

By default, the deduplication feature is enabled. This feature removes duplicate callees and keeps only the shortest path.

To disable deduplication and show all call paths:

```bash
call-cg4rs --no-dedup
```

### Options

To see all available options:

```bash
call-cg4rs -h
```

Output:
```
This is a bug detector for Rust.

Usage: cg4rs [OPTIONS] [CARGO_ARGS]...

Arguments:
  [CARGO_ARGS]...  Arguments passed to cargo rust-analyzer

Options:
      --show-all-funcs
          Show all functions
      --show-all-mir
          Show all MIR
      --emit-mir
          Emit MIR
      --entry-point <ENTRY_POINT>
          Entry point of the program
  -o, --output-dir <OUTPUT_DIR>
          Output directory
      --no-dedup
          No deduplication for call sites When enabled, keeps all call sites for the same caller-callee pair
      --find-callers <FIND_CALLERS>
          Find all callers of the specified function path(s) When specified, will output all functions that directly or indirectly call these functions. Can be specified multiple times for multiple targets
      --json-output
          Output the call graph as JSON format This provides machine-readable data for further processing
      --without-args
          Do not include generic type arguments in function paths When enabled, function paths will not include generic type parameters
      --timer-output <TIMER_OUTPUT>
          Output file for timing information When specified, will write detailed timing information to this file
      --cg-debug
          Enable debug mode When enabled, will print debug information
      --count-calls
          Generate call count statistics file (count-callgraph.txt)
      --count-debug
          Generate detailed call count debug file (count-callgraph-debug.txt)
      --manifest-path <MANIFEST_PATH>
          Path to the Cargo.toml file to use When specified, will use this manifest file instead of auto-detecting it
      --root-path <ROOT_PATH>
          Root path of the repository to analyze When specified, will use this as the base directory and automatically set manifest-path to <ROOT_PATH>/Cargo.toml
  -h, --help
          Print help
```

## Examples

### Analyzing a Test Directory

```bash
cd test_directory
call-cg4rs
# Call graph will be available at ./target/callgraph.txt
```

### Using Custom Output Directory

```bash
call-cg4rs -o ./custom_output
# Call graph will be available at ./custom_output/callgraph.txt
```

### Analyzing a Project in a Different Directory

You can analyze a Rust project located in a different directory without changing your current working directory:

```bash
# Using root-path (recommended)
call-cg4rs --root-path /path/to/project
# This will automatically find /path/to/project/Cargo.toml

# Using manifest-path directly
call-cg4rs --manifest-path /path/to/project/Cargo.toml
```

When using `--root-path`:
- The rust-toolchain.toml file is automatically copied to the specified directory
- All outputs (call graph, timing files, etc.) will be placed in the project's target directory
- The manifest path is automatically set to `<ROOT_PATH>/Cargo.toml`

This is especially useful for CI/CD pipelines or when analyzing multiple projects without changing directories.

### Outputting Call Graph in JSON Format

For machine-readable output that can be processed by other tools:

```bash
call-cg4rs --json-output
# JSON output will be available at ./target/callgraph.json
```

The JSON format includes detailed information about each caller and its callees, including:
- Function names
- Paths
- Version information (extracted from crate metadata)
- Call constraint depths
- DefPathHash identifier (unique hash for each function path)

Example JSON structure:
```json
[
  {
    "caller": {
      "name": "example_caller_name",
      "version": "1.0.0",
      "path": "example/path/to/caller",
      "constraint_depth": 3,
      "path_hash": "5a0e836d03d8617a"
    },
    "callee": [
      {
        "name": "example_callee_name_1",
        "version": "1.1.0",
        "path": "example/path/to/callee_1",
        "constraint_depth": 2,
        "path_hash": "8f219f8a15822e31"
      },
      {
        "name": "example_callee_name_2",
        "version": "1.2.0",
        "path": "example/path/to/callee_2",
        "constraint_depth": 1,
        "path_hash": "a7c5151b3c15fd4c"
      }
    ]
  }
]
```

This format is ideal for further processing or visualization with external tools.

### Finding All Callers of a Function

To find all functions that directly or indirectly call a specific function:

```bash
call-cg4rs --find-callers "std::collections::HashMap::insert"
```

This will generate a report of all callers in `./target/callers_1.txt`. The report includes each caller function along with its path constraints count, which represents the accumulated number of control flow constraints along the shortest calling path.

**Multiple Targets:**
You can specify multiple target functions by separating them with commas (no spaces):

```bash
call-cg4rs --find-callers "foo,bar,baz"
```

This will generate `callers_1.txt`, `callers_2.txt`, `callers_3.txt` for each target respectively.

You can also generate JSON reports with:

```bash
call-cg4rs --find-callers "foo,bar,baz" --json-output
```

Each target will produce a corresponding `callers_1.json`, `callers_2.json`, etc.

**Note:** Use English commas to separate multiple targets, and do not add spaces.

### Path Matching Behavior

You can use a partial path; the tool will match any function containing that substring. The matching behavior is as follows:

- If the path contains `<`, it will match against the full function path including generic parameters or the base path.
- If the path does not contain `<`, the tool will automatically remove all generic parameter sections (`::<...>`) from function paths before matching, providing clean results when searching for generic functions.

Examples:
```bash
# Match any DataStore::total_value regardless of generic parameters
call-cg4rs --find-callers "DataStore::total_value"

# Find callers for multiple targets at once
call-cg4rs --find-callers "DataStore::total_value,Product::discounted_price,std::collections::HashMap::new"

# Match a specific generic instantiation
call-cg4rs --find-callers "DataStore::<Electronics>::total_value"
```

### Performance Timing

The tool includes a built-in timing system that measures execution time of various components:

```bash
# Use default timing output file (./target/cg_timing.txt)
call-cg4rs

# Specify a custom timing output file
call-cg4rs --timer-output ./my_timing_report.txt
```

The timing report includes detailed information about:
- Overall execution time
- Time spent in various phases (collecting instances, monomorphization analysis)
- Time for individual operations (finding callers, deduplication)

This is useful for performance profiling and identifying bottlenecks in large codebases.

Example timing report:
```
Timer Report - 2023-05-15 14:30:45 +0800
------------------------------------------------------------
Timer Name                       | Count      | Total (ms)     | Avg (ms)       
------------------------------------------------------------
overall_execution                | 1          | 5430.25        | 5430.25        
rustc_driver_execution           | 1          | 5420.15        | 5420.15        
call_graph_analysis              | 1          | 4560.35        | 4560.35        
collect_generic_instances        | 1          | 245.78         | 245.78         
perform_mono_analysis            | 1          | 3950.45        | 3950.45        
compute_constraints              | 1532       | 825.32         | 0.54           
extract_function_call            | 1532       | 2450.67        | 1.60           
instance_callsites               | 1532       | 3285.43        | 2.14           
deduplicate_call_sites           | 1          | 120.43         | 120.43         
------------------------------------------------------------
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
   call-cg4rs
   ```
   This will generate a call graph at `./target/callgraph.txt`.

5. Try finding callers of specific functions, for example:
   ```bash
   # Find all callers of Product::discounted_price
   call-cg4rs --find-callers "Product::discounted_price"
   
   # Find all callers of DataStore::calculate_value_with_strategy
   call-cg4rs --find-callers "DataStore::calculate_value_with_strategy"
   ```

6. To see all possible call paths without deduplication:
   ```bash
   call-cg4rs --no-dedup
   ```

For detailed information about the test project structure and specific test cases, please refer to the documentation in the `test_callgraph` directory.

## Citing This Project

If you use this tool in academic research or other publications, please cite it using one of the following formats:

### BibTeX
```bibtex
@software{callgraph4rs,
  author = {Yin, Xizhe},
  title = {Rust Call Graph Generator},
  url = {https://github.com/xizheyin/callgraph4rs},
  year = {2025},
  description = {A tool for generating and analyzing call graphs for Rust projects}
}
```

## License

This project is dual-licensed under both:

- [Apache License 2.0](https://www.apache.org/licenses/LICENSE-2.0)
- [MIT License](https://opensource.org/licenses/MIT)

You may choose either license at your option.



