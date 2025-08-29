use rand::Rng;
use std::collections::HashMap;
use std::fmt::Debug;
use std::rc::Rc;

// 通用特征 - 产品接口
trait Product: Debug {
    fn name(&self) -> &str;
    fn price(&self) -> f64;
    fn category(&self) -> &str;
    fn discount(&self) -> f64 {
        0.0 // 默认没有折扣
    }

    fn discounted_price(&self) -> f64 {
        self.price() * (1.0 - self.discount())
    }
}

// 实现特征的具体产品类型
#[derive(Debug)]
struct Electronics {
    name: String,
    price: f64,
    warranty_months: u32,
}

impl Product for Electronics {
    fn name(&self) -> &str {
        &self.name
    }

    fn price(&self) -> f64 {
        self.price
    }

    fn category(&self) -> &str {
        "电子产品"
    }

    fn discount(&self) -> f64 {
        if self.warranty_months > 12 {
            0.05 // 长保修期产品有5%折扣
        } else {
            0.0
        }
    }
}

#[derive(Debug)]
struct Clothing {
    name: String,
    price: f64,
    size: String,
    season: Season,
}

#[derive(Debug)]
enum Season {
    Spring,
    Summer,
    Autumn,
    Winter,
}

impl Product for Clothing {
    fn name(&self) -> &str {
        &self.name
    }

    fn price(&self) -> f64 {
        self.price
    }

    fn category(&self) -> &str {
        "服装"
    }

    fn discount(&self) -> f64 {
        match self.season {
            Season::Winter => 0.1,  // 冬季服装打9折
            Season::Summer => 0.15, // 夏季服装打85折
            _ => 0.0,
        }
    }
}

// 泛型数据仓库
struct DataStore<T> {
    items: Vec<T>,
    name: String,
}

impl<T> DataStore<T> {
    fn new(name: &str) -> Self {
        DataStore {
            items: Vec::new(),
            name: name.to_string(),
        }
    }

    fn add(&mut self, item: T) {
        self.items.push(item);
    }

    fn len(&self) -> usize {
        self.items.len()
    }
}

impl<T: Product> DataStore<T> {
    fn total_value(&self) -> f64 {
        self.calculate_value_with_strategy(|item| item.price())
    }

    fn total_discounted_value(&self) -> f64 {
        self.calculate_value_with_strategy(|item| item.discounted_price())
    }

    fn calculate_value_with_strategy<F>(&self, strategy: F) -> f64
    where
        F: Fn(&T) -> f64,
    {
        let mut total = 0.0;
        for item in &self.items {
            let item_value = strategy(item);
            total += item_value;
            log_calculation(item.name(), item_value);
        }
        total
    }

    fn find_most_expensive(&self) -> Option<&T> {
        if self.items.is_empty() {
            return None;
        }

        self.items
            .iter()
            .max_by(|a, b| a.price().partial_cmp(&b.price()).unwrap())
    }
}

// 库存管理系统
struct InventoryManager {
    electronics: DataStore<Electronics>,
    clothing: DataStore<Clothing>,
    sales_data: HashMap<String, usize>,
}

impl InventoryManager {
    fn new() -> Self {
        InventoryManager {
            electronics: DataStore::new("电子产品库存"),
            clothing: DataStore::new("服装库存"),
            sales_data: HashMap::new(),
        }
    }

    fn add_electronic(&mut self, item: Electronics) {
        self.electronics.add(item);
    }

    fn add_clothing(&mut self, item: Clothing) {
        self.clothing.add(item);
    }

    fn record_sale(&mut self, product_name: &str, quantity: usize) {
        let entry = self.sales_data.entry(product_name.to_string()).or_insert(0);
        *entry += quantity;
    }

    fn generate_inventory_report(&self) -> String {
        let mut report = String::new();

        report.push_str("=== 库存报告 ===\n");
        report.push_str(&format!("电子产品: {} 件\n", self.electronics.len()));
        report.push_str(&format!("服装: {} 件\n", self.clothing.len()));

        // 计算电子产品总价值
        let electronics_value = self.electronics.total_value();
        let electronics_discounted = self.electronics.total_discounted_value();

        // 计算服装总价值
        let clothing_value = self.clothing.total_value();
        let clothing_discounted = self.clothing.total_discounted_value();

        // 添加价值信息到报告
        report.push_str("\n=== 价值报告 ===\n");
        report.push_str(&format!("电子产品总价值: ¥{:.2}\n", electronics_value));
        report.push_str(&format!(
            "电子产品折后价值: ¥{:.2}\n",
            electronics_discounted
        ));
        report.push_str(&format!("服装总价值: ¥{:.2}\n", clothing_value));
        report.push_str(&format!("服装折后价值: ¥{:.2}\n", clothing_discounted));

        // 查找最贵的产品
        if let Some(most_expensive_electronic) = self.electronics.find_most_expensive() {
            report.push_str(&format!(
                "\n最贵的电子产品: {} (¥{:.2})\n",
                most_expensive_electronic.name(),
                most_expensive_electronic.price()
            ));
        }

        if let Some(most_expensive_clothing) = self.clothing.find_most_expensive() {
            report.push_str(&format!(
                "最贵的服装: {} (¥{:.2})\n",
                most_expensive_clothing.name(),
                most_expensive_clothing.price()
            ));
        }

        // 生成销售数据报告
        self.append_sales_report(&mut report);

        report
    }

    fn append_sales_report(&self, report: &mut String) {
        report.push_str("\n=== 销售报告 ===\n");

        if self.sales_data.is_empty() {
            report.push_str("暂无销售数据\n");
            return;
        }

        let mut total_sales = 0;

        // 使用函数指针处理数据
        let mut process_and_sum = |name: &str, quantity: &usize| -> usize {
            report.push_str(&format!("{}: {} 件\n", name, quantity));
            *quantity
        };

        for (name, quantity) in &self.sales_data {
            total_sales += process_and_sum(name, quantity);
        }

        report.push_str(&format!("\n总销售量: {} 件\n", total_sales));

        // 使用闭包进行销售额估算
        let estimate_revenue = |sales: usize| -> f64 {
            let average_price = 199.99;
            sales as f64 * average_price
        };

        let estimated_revenue = estimate_revenue(total_sales);
        report.push_str(&format!("估计销售额: ¥{:.2}\n", estimated_revenue));
    }

    // 静态方法：创建一个包含示例数据的管理器
    fn create_example() -> Self {
        let mut manager = InventoryManager::new();

        // 添加一些电子产品
        manager.add_electronic(Electronics {
            name: "智能手机".to_string(),
            price: 3999.99,
            warranty_months: 24,
        });

        manager.add_electronic(Electronics {
            name: "笔记本电脑".to_string(),
            price: 5999.99,
            warranty_months: 12,
        });

        // 添加一些服装
        manager.add_clothing(Clothing {
            name: "冬季外套".to_string(),
            price: 299.99,
            size: "L".to_string(),
            season: Season::Winter,
        });

        manager.add_clothing(Clothing {
            name: "T恤".to_string(),
            price: 99.99,
            size: "M".to_string(),
            season: Season::Summer,
        });

        // 记录一些销售数据
        manager.record_sale("智能手机", 5);
        manager.record_sale("冬季外套", 3);

        manager
    }
}

// 模拟数据生成器
struct DataGenerator;

impl DataGenerator {
    fn generate_random_products<R: Rng>(rng: &mut R, count: usize) -> Vec<Rc<dyn Product>> {
        let mut products: Vec<Rc<dyn Product>> = Vec::with_capacity(count);

        for i in 0..count {
            if rng.gen_bool(0.5) {
                // 创建电子产品
                let electronic = Electronics {
                    name: format!("电子产品 #{}", i),
                    price: rng.gen_range(100.0..5000.0),
                    warranty_months: rng.gen_range(3..36),
                };
                products.push(Rc::new(electronic));
            } else {
                // 创建服装
                let season = match rng.gen_range(0..4) {
                    0 => Season::Spring,
                    1 => Season::Summer,
                    2 => Season::Autumn,
                    _ => Season::Winter,
                };

                let clothing = Clothing {
                    name: format!("服装 #{}", i),
                    price: rng.gen_range(50.0..500.0),
                    size: ["S", "M", "L", "XL"][rng.gen_range(0..4)].to_string(),
                    season,
                };
                products.push(Rc::new(clothing));
            }
        }

        products
    }

    fn analyze_products(products: &[Rc<dyn Product>]) -> HashMap<&str, usize> {
        let mut categories = HashMap::new();

        for product in products {
            let category = product.category();
            let entry = categories.entry(category).or_insert(0);
            *entry += 1;
        }

        categories
    }
}

// 日志函数
fn log_calculation(item_name: &str, value: f64) {
    if cfg!(debug_assertions) {
        println!("计算 {} 的价值: ¥{:.2}", item_name, value);
    }
}

// 主函数
fn main() {
    println!("创建库存管理器...");
    let manager = InventoryManager::create_example();

    println!("\n生成库存报告...");
    let report = manager.generate_inventory_report();
    println!("{}", report);

    println!("\n生成随机产品数据...");
    let mut rng = rand::thread_rng();
    let random_products = DataGenerator::generate_random_products(&mut rng, 10);

    println!("随机产品分类统计:");
    let category_counts = DataGenerator::analyze_products(&random_products);
    for (category, count) in category_counts {
        println!("{}: {} 件", category, count);
    }

    println!("\n折扣价格计算:");
    for product in &random_products {
        println!(
            "{}: 原价 ¥{:.2}, 折后价 ¥{:.2}, 折扣 {:.1}%",
            product.name(),
            product.price(),
            product.discounted_price(),
            product.discount() * 100.0
        );
    }
}
