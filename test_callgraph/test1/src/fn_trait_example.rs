// 演示 Fn / FnMut / FnOnce 三种调用方式，便于解析测试

// 若希望解析到具体函数实现，使用函数项（如 inc/echo_str）赋值到 trait 对象
fn inc(x: i32) -> i32 {
    x + 1
}
fn double(x: i32) -> i32 {
    x * 2
}
fn echo_str(x: &str) -> &str {
    x
}

// 泛型：测试 FnOnce（闭包消费捕获的值）
fn call_once<F: FnOnce() -> i32>(f: F) -> i32 {
    f()
}

fn xxtest1() {
    // 1) Fn：整数签名，能直接解析到具体函数 inc/double
    let f_inc: &dyn Fn(i32) -> i32 = &inc;
    let f_double: &dyn Fn(i32) -> i32 = &double;
    println!("Fn: f_inc(5) = {}", f_inc(5));
    println!("Fn: f_double(5) = {}", f_double(5));
}

fn xxtest2() {
    // 2) Fn：字符串签名 (&str) -> &str，直接解析到 echo_str
    let f_str: &dyn Fn(&str) -> &str = &echo_str;
    println!("Fn: f_str(hello) = {}", f_str("hello"));
}

fn xxtest3() {
    // 3) FnMut：使用函数项和闭包两种形式
    // 3.1 使用函数项作为 FnMut（函数项/指针实现 Fn/FnMut/FnOnce）
    let mut fm_func: Box<dyn FnMut(i32) -> i32> = Box::new(inc);
    println!("FnMut(func): fm_func(3) = {}", fm_func(3));
    println!("FnMut(func): fm_func(4) = {}", fm_func(4));
}

fn xxtest4() {
    // 3.2 使用捕获状态的闭包作为 FnMut
    let mut total = 0;
    let mut fm_closure: Box<dyn FnMut(i32) -> i32> = Box::new(move |x| {
        total += x;
        total
    });
    println!("FnMut(closure): fm_closure(2) = {}", fm_closure(2));
    println!("FnMut(closure): fm_closure(5) = {}", fm_closure(5));
}

pub fn xxmain() {
    println!("=== Fn/FnMut/FnOnce 示例 ===");
    xxtest1();
    xxtest2();
    xxtest3();
    xxtest4();

    // 4) FnOnce：闭包消费捕获的值，只能调用一次
    let v = vec![1, 2, 3];
    let once_sum = move || v.into_iter().sum::<i32>(); // 消费 v => FnOnce
    let res_once = call_once(once_sum);
    println!("FnOnce(closure move/consume): sum = {}", res_once);

    // 也可用函数项作为 FnOnce（尽管它同时实现 Fn/FnMut/FnOnce）
    let res_once_func = call_once(|| inc(10));
    println!("FnOnce(func): inc(10) = {}", res_once_func);

    println!("=== Fn/FnMut/FnOnce 示例完成 ===");
}
