// 函数指针调用示例：测试静态函数指针与可变更新
// 目标：让调用图识别以下几类调用
// 1) 直接通过函数名调用（静态）
// 2) 通过函数指针变量调用（Fn 指针）
// 3) 通过数组/向量中的函数指针遍历调用
// 4) 通过结构体字段持有函数指针并调用
// 5) 在作用域内切换函数指针指向不同函数并调用

// 一些被调用的目标函数
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
fn example_str(x: &str) -> &str {
    x
}

// 结构体持有函数指针
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

// 复杂一些：返回函数指针的工厂
fn make_op(kind: &str) -> fn(i32) -> i32 {
    match kind {
        "add" => add_one,
        "mul" => times_two,
        "sqr" => square,
        _ => negate,
    }
}

// 展示多种函数指针相关调用
pub fn main() {
    println!("=== 函数指针示例 ===");

    // 1) 直接调用
    let a = add_one(10); // 直接调用 add_one
    println!("add_one(10) = {}", a);

    // 2) 通过函数指针变量调用
    let mut fp: fn(i32) -> i32 = times_two; // 指向 times_two
    let b = fp(10);
    println!("fp(10) [times_two] = {}", b);

    // 切换指向 square
    fp = square;
    let c = fp(10);
    println!("fp(10) [square] = {}", c);

    // 3) 通过集合中的函数指针遍历调用
    let ops: [fn(i32) -> i32; 4] = [add_one, times_two, square, negate];
    for (i, op) in ops.iter().enumerate() {
        let v = op(5);
        println!("ops[{}](5) = {}", i, v);
    }

    // 4) 结构体字段持有函数指针并调用
    let holder_add = OpHolder::new(add_one, example_str);
    let holder_mul = OpHolder::new(times_two, example_str);
    println!("holder_add.apply(7) = {}", holder_add.apply(7));
    println!("holder_mul.apply(7) = {}", holder_mul.apply(7));
    println!("holder_add.apply_str(\"hello\") = {}", holder_add.apply_str("hello"));

    // 5) 工厂返回函数指针并调用
    let f_add = make_op("add");
    let f_mul = make_op("mul");
    let f_sqr = make_op("sqr");
    let f_neg = make_op("other");
    println!("make_op(add)(3) = {}", f_add(3));
    println!("make_op(mul)(3) = {}", f_mul(3));
    println!("make_op(sqr)(3) = {}", f_sqr(3));
    println!("make_op(neg)(3) = {}", f_neg(3));

    // 6) 使用高阶函数将函数指针作为参数
    fn apply_twice(f: fn(i32) -> i32, x: i32) -> i32 {
        f(f(x))
    }
    println!("apply_twice(square, 2) = {}", apply_twice(square, 2));

    // 7) 与闭包并列（对比：闭包是 trait 对象，函数指针是具体类型 fn）
    let closure = |x: i32| x + 3;
    let res_closure = closure(4);
    let res_fn_ptr = (add_one)(4);
    println!("closure(4) = {}, add_one(4) = {}", res_closure, res_fn_ptr);

    println!("=== 函数指针示例完成 ===");
}
