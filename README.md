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
Default `cargo clean`, you can pass `--no-clean` to off the step.

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
      --timer-output <TIMER_OUTPUT> Output file for timing information
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
call-cg --find-callers-of "std::collections::HashMap::insert"
```

This will generate a report of all callers in `./target/callers.txt`.

You can also generate a JSON format report by combining with the `--json-output` option:

```bash
call-cg --find-callers-of "std::collections::HashMap::insert" --json-output
```

This will output the callers information in JSON format to `./target/callers.json`, which is useful for programmatic processing or visualization.

Example JSON structure for callers:
```json
{
  "target": "std::collections::HashMap::insert",
  "total_callers": 3,
  "callers": [
    {
      "name": "my_module::my_function",
      "version": "1.0.0",
      "path": "my_crate::my_module::my_function",
      "path_hash": "7f21c985b3ab412c"
    },
    {
      "name": "another_function",
      "version": "0.5.2",
      "path": "other_crate::another_function",
      "path_hash": "d23f91e6a0189f50"
    }
  ]
}
```

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

# Find callers using the exact DefPathHash (more precise than path matching)
call-cg --find-callers-by-hash "8f219f8a15822e31"
```

### Finding Functions by DefPathHash

For precise function lookup, you can use the `--find-callers-by-hash` option with the DefPathHash value:

```bash
call-cg --find-callers-by-hash "8f219f8a15822e31"
```

This approach provides a more reliable way to find functions than using paths, especially when:
- The same function name appears in multiple modules
- Working with complex generic instantiations
- Dealing with mangled function names

You can find the DefPathHash in the standard output of any call graph report - each function path
is displayed with its hash in square brackets, e.g., `DataStore::total_value [8f219f8a15822e31]`.

This can also be combined with the `--json-output` option to get a JSON representation:

```bash
call-cg --find-callers-by-hash "8f219f8a15822e31" --json-output
```

The results will be available in `./target/callers_by_hash.txt` or `./target/callers_by_hash.json`.

> **Note**: The `--find-callers-of` and `--find-callers-by-hash` options are mutually exclusive. If both are specified, `--find-callers-of` will be used and `--find-callers-by-hash` will be ignored.

### Performance Timing

The tool includes a built-in timing system that measures execution time of various components:

```bash
# Use default timing output file (./target/cg_timing.txt)
call-cg

# Specify a custom timing output file
call-cg --timer-output ./my_timing_report.txt
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



