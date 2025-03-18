
install `cg`, `cargo-cg`, `call-cg` by
```
cargo install --path .
```

and change into the test directory, and type
```
call-cg
```
it print the call graph in `./target/callgraph.txt`

`call-cg --deduplicate` will deduplicate same callees, choosing the shortest path to it.



`call-cg -h` will print:
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
      --deduplicate                Deduplicate call sites for the same caller-callee pair When enabled, only keeps the call site with the minimum constraints
  -h, --help                       Print help
```