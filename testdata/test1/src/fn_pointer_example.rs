// Function pointer call examples: test static pointers and mutable updates
// Goal: let the call graph recognize the following call patterns
// 1) Direct calls by function name
// 2) Calls through function pointer variables
// 3) Iteration over function pointers stored in arrays or vectors
// 4) Calls through struct fields that hold function pointers
// 5) Rebinding a function pointer to different functions within a scope

// Some target functions that will be called
fn add_one(x: i32) -> i32 {
    x + 1
}
fn times_two(x: i32) -> i32 {
    x * 2
}
fn square(x: i32) -> i32 {
    x * x
}
fn negate(x: i32) -> i32 {
    -x
}
fn plus_hundred(x: i32) -> i32 {
    x + 100
}
fn minus_hundred(x: i32) -> i32 {
    x - 100
}
fn generic_slot_reader<T>(slot: Option<&mut Option<T>>) -> *const T {
    match slot {
        Some(slot) => slot.as_ref().map_or(std::ptr::null(), |value| value as *const T),
        None => std::ptr::null(),
    }
}
fn generic_ptr_sink<T>(_ptr: *mut T) {}
fn example_str(x: &str) -> &str {
    x
}

// Another string function with the same signature for direct resolution
fn id_str(x: &str) -> &str {
    x
}

// Struct holding function pointers
struct OpHolder {
    op: fn(i32) -> i32,
    op_str: fn(&str) -> &str,
}

impl OpHolder {
    fn new(op: fn(i32) -> i32, op_str: fn(&str) -> &str) -> Self {
        Self { op, op_str }
    }
    fn apply(&self, x: i32) -> i32 {
        (self.op)(x)
    }

    fn apply_str<'a>(&self, x: &'a str) -> &'a str {
        (self.op_str)(x)
    }
}

// Slightly more complex: a factory returning function pointers
fn make_op(kind: &str) -> fn(i32) -> i32 {
    match kind {
        "add" => add_one,
        "mul" => times_two,
        _ => square,
    }
}

// Factory returning string function pointers with signature (&str) -> &str
fn make_str_op(kind: &str) -> fn(&str) -> &str {
    match kind {
        "echo" => example_str,
        "id" => id_str,
        _ => example_str,
    }
}

fn test1() {
    // 8) Directly resolvable string function pointer call
    let fp_str: fn(&str) -> &str = example_str; // Directly points to example_str
    let s1 = fp_str("world");
    println!("fp_str(\"world\") [example_str] = {}", s1);
}

fn test2() {
    // 9) Directly resolvable string function pointer call
    let fp_str: fn(&str) -> &str = id_str; // Directly points to id_str
    let s2 = fp_str("rust");
    println!("fp_str(\"rust\") [id_str] = {}", s2);
}

fn test3() {
    // 10) Iteration over string function pointers in a collection
    let s_ops: [fn(&str) -> &str; 2] = [example_str, id_str];
    for (i, op) in s_ops.iter().enumerate() {
        let v = op("hi");
        println!("s_ops[{}](\"hi\") = {}", i, v);
    }
}

fn test4() {
    // 11) Call string function pointers returned by the factory
    let f_echo = make_str_op("echo");
    let f_id = make_str_op("id");
    println!("make_str_op(echo)(\"hello\") = {}", f_echo("hello"));
    println!("make_str_op(id)(\"hello\") = {}", f_id("hello"));
}

fn test5() {
    // 12) Call a single string function pointer returned by the factory
    let f_echo = make_str_op("echo");
    println!("make_str_op(echo)(\"hello\") = {}", f_echo("hello"));
}

// 13) Assign the function pointer in predecessor blocks, then call it after control flow joins.
fn branch_selected_op(flag: bool) -> fn(i32) -> i32 {
    let selected: fn(i32) -> i32;
    if flag {
        selected = add_one;
    } else {
        selected = times_two;
    }
    selected
}

fn call_after_cfg_join(flag: bool, x: i32) -> i32 {
    let fp: fn(i32) -> i32;
    if flag {
        fp = add_one;
    } else {
        fp = square;
    }

    let forwarded = fp;
    forwarded(x)
}

// 14) Producer-consumer patterns: direct producer and passthrough wrapper
fn make_bonus_op() -> fn(i32) -> i32 {
    plus_hundred
}

fn passthrough_op(op: fn(i32) -> i32) -> fn(i32) -> i32 {
    op
}

fn make_offset_closure() -> impl Fn(i32) -> i32 {
    |x| x + 10
}

fn invoke_slot_reader<T>(reader: fn(Option<&mut Option<T>>) -> *const T, slot: Option<&mut Option<T>>) -> *const T {
    reader(slot)
}

fn invoke_ptr_sink<T>(sink: fn(*mut T), ptr: *mut T) {
    sink(ptr);
}

fn test6() {
    let join_add = call_after_cfg_join(true, 4);
    let join_square = call_after_cfg_join(false, 4);
    let selected_add = branch_selected_op(true)(5);
    let selected_mul = branch_selected_op(false)(5);

    println!("call_after_cfg_join(true, 4) = {}", join_add);
    println!("call_after_cfg_join(false, 4) = {}", join_square);
    println!("branch_selected_op(true)(5) = {}", selected_add);
    println!("branch_selected_op(false)(5) = {}", selected_mul);
}

fn test7_bonus() {
    let produced = make_bonus_op();
    println!("make_bonus_op()(7) = {}", produced(7));
}

fn test8_passthrough() {
    let forwarded = passthrough_op(minus_hundred);
    println!("passthrough_op(minus_hundred)(7) = {}", forwarded(7));
}

fn test9_closure_producer() {
    let returned_closure = make_offset_closure();
    println!("make_offset_closure()(7) = {}", returned_closure(7));
}

fn test10_generic_slot_reader() {
    let mut slot = Some(std::cell::Cell::new((1_u64, 2_u64)));
    let ptr = invoke_slot_reader(generic_slot_reader::<std::cell::Cell<(u64, u64)>>, Some(&mut slot));
    println!("invoke_slot_reader(cell) produced null? {}", ptr.is_null());
}

fn test11_generic_ptr_sink() {
    let mut byte = 7_u8;
    invoke_ptr_sink(generic_ptr_sink::<u8>, &mut byte as *mut u8);
    println!("invoke_ptr_sink(byte) ran");
}

// Show several function-pointer-related call patterns
pub fn main() {
    println!("=== Function Pointer Example ===");

    // 1) Direct call
    let a = add_one(10); // Direct call to add_one
    println!("add_one(10) = {}", a);

    // 2) Call through a function pointer variable
    let mut fp: fn(i32) -> i32 = times_two; // Points to times_two
    let b = fp(10);
    println!("fp(10) [times_two] = {}", b);

    // Rebind to square
    fp = square;
    let c = fp(10);
    println!("fp(10) [square] = {}", c);

    // 3) Iterate over function pointers in a collection
    let ops: [fn(i32) -> i32; 3] = [add_one, times_two, square];
    for (i, op) in ops.iter().enumerate() {
        let v = op(5);
        println!("ops[{}](5) = {}", i, v);
    }

    // 4) Struct fields holding function pointers
    let holder_add = OpHolder::new(add_one, example_str);
    let holder_mul = OpHolder::new(times_two, example_str);
    println!("holder_add.apply(7) = {}", holder_add.apply(7));
    println!("holder_mul.apply(7) = {}", holder_mul.apply(7));
    println!("holder_add.apply_str(\"hello\") = {}", holder_add.apply_str("hello"));

    // 5) Function pointers returned by a factory
    let f_add = make_op("add");
    let f_mul = make_op("mul");
    let f_sqr = make_op("sqr");
    let f_neg = make_op("other");
    println!("make_op(add)(3) = {}", f_add(3));
    println!("make_op(mul)(3) = {}", f_mul(3));
    println!("make_op(sqr)(3) = {}", f_sqr(3));
    println!("make_op(neg)(3) = {}", f_neg(3));

    // 6) Pass a function pointer into a higher-order function
    fn apply_twice(f: fn(i32) -> i32, x: i32) -> i32 {
        f(f(x))
    }
    println!("apply_twice(square, 2) = {}", apply_twice(square, 2));

    // 7) Compare function pointers with closures
    let closure = |x: i32| x + 3;
    let res_closure = closure(4);
    let res_fn_ptr = (add_one)(4);
    println!("closure(4) = {}, add_one(4) = {}", res_closure, res_fn_ptr);

    // 13) Cross-block function pointer propagation through a CFG join
    test6();
    test7_bonus();
    test8_passthrough();
    test9_closure_producer();
    test10_generic_slot_reader();
    test11_generic_ptr_sink();

    println!("=== Function Pointer Example Complete ===");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cross_block_fnptr_resolution_behavior() {
        assert_eq!(call_after_cfg_join(true, 4), 5);
        assert_eq!(call_after_cfg_join(false, 4), 16);
        assert_eq!(branch_selected_op(true)(5), 6);
        assert_eq!(branch_selected_op(false)(5), 10);
    }

    #[test]
    fn test_producer_consumer_runtime_behavior() {
        assert_eq!(make_bonus_op()(7), 107);
        assert_eq!(passthrough_op(minus_hundred)(7), -93);
        assert_eq!(make_offset_closure()(7), 17);
    }

    #[test]
    fn test_generic_fnptr_runtime_behavior() {
        let mut slot = Some(std::cell::Cell::new((3_u64, 4_u64)));
        let ptr = invoke_slot_reader(generic_slot_reader::<std::cell::Cell<(u64, u64)>>, Some(&mut slot));
        assert!(!ptr.is_null());

        let mut byte = 9_u8;
        invoke_ptr_sink(generic_ptr_sink::<u8>, &mut byte as *mut u8);
        assert_eq!(byte, 9);
    }
}
