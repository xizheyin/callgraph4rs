// Demonstrate Fn / FnMut / FnOnce call styles for analysis tests

// Use function items (such as inc/echo_str) when direct resolution is desired
fn inc(x: i32) -> i32 {
    x + 1
}
fn double(x: i32) -> i32 {
    x * 2
}
fn add_offset(x: i32, offset: i32) -> i32 {
    x + offset
}
fn echo_str(x: &str) -> &str {
    x
}

fn call_with_fn<F: Fn(i32) -> i32>(f: F, x: i32) -> i32 {
    f(x)
}

fn call_nested_fn<F: Fn(i32) -> i32>(f: F, x: i32) -> i32 {
    call_with_fn(f, x)
}

// Generic helper for testing FnOnce closures that consume captured values
fn call_once<F: FnOnce() -> i32>(f: F) -> i32 {
    f()
}

fn xxtest1() {
    // 1) Fn: integer signature, directly resolvable to inc/double
    let f_inc: &dyn Fn(i32) -> i32 = &inc;
    let f_double: &dyn Fn(i32) -> i32 = &double;
    println!("Fn: f_inc(5) = {}", f_inc(5));
    println!("Fn: f_double(5) = {}", f_double(5));
}

fn xxtest2() {
    // 2) Fn: string signature (&str) -> &str, directly resolvable to echo_str
    let f_str: &dyn Fn(&str) -> &str = &echo_str;
    println!("Fn: f_str(hello) = {}", f_str("hello"));
}

fn xxtest3() {
    // 3) FnMut: demonstrate both function items and closures
    // 3.1 Use a function item as FnMut (function items implement Fn/FnMut/FnOnce)
    let mut fm_func: Box<dyn FnMut(i32) -> i32> = Box::new(inc);
    println!("FnMut(func): fm_func(3) = {}", fm_func(3));
    println!("FnMut(func): fm_func(4) = {}", fm_func(4));
}

fn xxtest4() {
    // 3.2 Use a stateful closure as FnMut
    let mut total = 0;
    let mut fm_closure: Box<dyn FnMut(i32) -> i32> = Box::new(move |x| {
        total += x;
        total
    });
    println!("FnMut(closure): fm_closure(2) = {}", fm_closure(2));
    println!("FnMut(closure): fm_closure(5) = {}", fm_closure(5));
}

fn xxtest5() {
    let closure = |x| inc(x);
    println!(
        "Higher-order closure arg: call_with_fn(closure, 6) = {}",
        call_with_fn(closure, 6)
    );
}

fn xxtest6() {
    let offset = 10;
    let closure = |x| add_offset(x, offset);
    println!(
        "Capture closure arg: call_with_fn(closure, 6) = {}",
        call_with_fn(closure, 6)
    );
}

fn xxtest7() {
    let offset = 20;
    let closure = |x| add_offset(x, offset);
    println!(
        "Nested closure arg: call_nested_fn(closure, 6) = {}",
        call_nested_fn(closure, 6)
    );
}

pub fn xxmain() {
    println!("=== Fn/FnMut/FnOnce Example ===");
    xxtest1();
    xxtest2();
    xxtest3();
    xxtest4();
    xxtest5();
    xxtest6();
    xxtest7();

    // 4) FnOnce: closure consumes captured values and can be called only once
    let v = vec![1, 2, 3];
    let once_sum = move || v.into_iter().sum::<i32>(); // Consumes v => FnOnce
    let res_once = call_once(once_sum);
    println!("FnOnce(closure move/consume): sum = {}", res_once);

    // A function item can also be used as FnOnce
    let res_once_func = call_once(|| inc(10));
    println!("FnOnce(func): inc(10) = {}", res_once_func);

    println!("=== Fn/FnMut/FnOnce Example Complete ===");
}
