use rand::Rng;
use std::collections::HashMap;
use std::fmt::Debug;
use std::rc::Rc;

mod dyn_example;
mod external_trait_example;
mod fn_pointer_example;
mod fn_trait_example;
mod manual_serde;
mod ultra_simple_serde;
// mod serde_import_only;

// Generic trait - product interface
trait Product: Debug {
    fn name(&self) -> &str;
    fn price(&self) -> f64;
    fn category(&self) -> &str;
    fn discount(&self) -> f64 {
        0.0 // No discount by default
    }

    fn discounted_price(&self) -> f64 {
        self.price() * (1.0 - self.discount())
    }
}

// Concrete product types implementing the trait
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
        "Electronics"
    }

    fn discount(&self) -> f64 {
        if self.warranty_months > 12 {
            0.05 // Products with longer warranties get a 5% discount
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
        "Clothing"
    }

    fn discount(&self) -> f64 {
        match self.season {
            Season::Winter => 0.1,  // Winter clothing gets a 10% discount
            Season::Summer => 0.15, // Summer clothing gets a 15% discount
            _ => 0.0,
        }
    }
}

// Generic data store
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

// Inventory management system
struct InventoryManager {
    electronics: DataStore<Electronics>,
    clothing: DataStore<Clothing>,
    sales_data: HashMap<String, usize>,
}

impl InventoryManager {
    fn new() -> Self {
        InventoryManager {
            electronics: DataStore::new("Electronics inventory"),
            clothing: DataStore::new("Clothing inventory"),
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

        report.push_str("=== Inventory Report ===\n");
        report.push_str(&format!("Electronics: {} items\n", self.electronics.len()));
        report.push_str(&format!("Clothing: {} items\n", self.clothing.len()));

        // Calculate total electronics value
        let electronics_value = self.electronics.total_value();
        let electronics_discounted = self.electronics.total_discounted_value();

        // Calculate total clothing value
        let clothing_value = self.clothing.total_value();
        let clothing_discounted = self.clothing.total_discounted_value();

        // Add value information to the report
        report.push_str("\n=== Value Report ===\n");
        report.push_str(&format!("Total electronics value: ¥{:.2}\n", electronics_value));
        report.push_str(&format!(
            "Discounted electronics value: ¥{:.2}\n",
            electronics_discounted
        ));
        report.push_str(&format!("Total clothing value: ¥{:.2}\n", clothing_value));
        report.push_str(&format!("Discounted clothing value: ¥{:.2}\n", clothing_discounted));

        // Find the most expensive products
        if let Some(most_expensive_electronic) = self.electronics.find_most_expensive() {
            report.push_str(&format!(
                "\nMost expensive electronic item: {} (¥{:.2})\n",
                most_expensive_electronic.name(),
                most_expensive_electronic.price()
            ));
        }

        if let Some(most_expensive_clothing) = self.clothing.find_most_expensive() {
            report.push_str(&format!(
                "Most expensive clothing item: {} (¥{:.2})\n",
                most_expensive_clothing.name(),
                most_expensive_clothing.price()
            ));
        }

        // Generate the sales report
        self.append_sales_report(&mut report);

        report
    }

    fn append_sales_report(&self, report: &mut String) {
        report.push_str("\n=== Sales Report ===\n");

        if self.sales_data.is_empty() {
            report.push_str("No sales data available\n");
            return;
        }

        let mut total_sales = 0;

        // Use a function pointer-style closure to process data
        let mut process_and_sum = |name: &str, quantity: &usize| -> usize {
            report.push_str(&format!("{}: {} items\n", name, quantity));
            *quantity
        };

        for (name, quantity) in &self.sales_data {
            total_sales += process_and_sum(name, quantity);
        }

        report.push_str(&format!("\nTotal sales volume: {} items\n", total_sales));

        // Estimate revenue with a closure
        let estimate_revenue = |sales: usize| -> f64 {
            let average_price = 199.99;
            sales as f64 * average_price
        };

        let estimated_revenue = estimate_revenue(total_sales);
        report.push_str(&format!("Estimated revenue: ¥{:.2}\n", estimated_revenue));
    }

    // Static helper that creates a manager with sample data
    fn create_example() -> Self {
        let mut manager = InventoryManager::new();

        // Add a few electronic products
        manager.add_electronic(Electronics {
            name: "Smartphone".to_string(),
            price: 3999.99,
            warranty_months: 24,
        });

        manager.add_electronic(Electronics {
            name: "Laptop".to_string(),
            price: 5999.99,
            warranty_months: 12,
        });

        // Add a few clothing items
        manager.add_clothing(Clothing {
            name: "Winter Coat".to_string(),
            price: 299.99,
            size: "L".to_string(),
            season: Season::Winter,
        });

        manager.add_clothing(Clothing {
            name: "T-Shirt".to_string(),
            price: 99.99,
            size: "M".to_string(),
            season: Season::Summer,
        });

        // Record some sales data
        manager.record_sale("Smartphone", 5);
        manager.record_sale("Winter Coat", 3);

        manager
    }
}

// Synthetic data generator
struct DataGenerator;

impl DataGenerator {
    fn generate_random_products<R: Rng>(rng: &mut R, count: usize) -> Vec<Rc<dyn Product>> {
        let mut products: Vec<Rc<dyn Product>> = Vec::with_capacity(count);

        for i in 0..count {
            if rng.gen_bool(0.5) {
                // Create an electronic product
                let electronic = Electronics {
                    name: format!("Electronic Item #{}", i),
                    price: rng.gen_range(100.0..5000.0),
                    warranty_months: rng.gen_range(3..36),
                };
                products.push(Rc::new(electronic));
            } else {
                // Create a clothing item
                let season = match rng.gen_range(0..4) {
                    0 => Season::Spring,
                    1 => Season::Summer,
                    2 => Season::Autumn,
                    _ => Season::Winter,
                };

                let clothing = Clothing {
                    name: format!("Clothing Item #{}", i),
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

// Logging helper
fn log_calculation(item_name: &str, value: f64) {
    if cfg!(debug_assertions) {
        println!("Calculated value for {}: ¥{:.2}", item_name, value);
    }
}

struct DropTracer {
    id: i32,
}

fn drop_sink(id: i32) -> i32 {
    id + 1
}

impl Drop for DropTracer {
    fn drop(&mut self) {
        let observed = drop_sink(self.id);
        println!("DropTracer observed {}", observed);
    }
}

fn trigger_scope_drop() {
    let _tracer = DropTracer { id: 41 };
}

// Main entry point
fn main() {
    println!("Creating inventory manager...");
    let manager = InventoryManager::create_example();

    println!("\nGenerating inventory report...");
    let report = manager.generate_inventory_report();
    println!("{}", report);

    println!("\nGenerating random product data...");
    let mut rng = rand::thread_rng();
    let random_products = DataGenerator::generate_random_products(&mut rng, 10);

    println!("Random product category counts:");
    let category_counts = DataGenerator::analyze_products(&random_products);
    for (category, count) in category_counts {
        println!("{}: {} items", category, count);
    }

    println!("\nDiscounted price calculations:");
    for product in &random_products {
        println!(
            "{}: original ¥{:.2}, discounted ¥{:.2}, discount {:.1}%",
            product.name(),
            product.price(),
            product.discounted_price(),
            product.discount() * 100.0
        );
    }

    println!("\n=== Running dyn trait dispatch example ===");
    dyn_example::main();

    println!("\n=== External crate trait dynamic dispatch example ===");
    external_trait_example::main();

    println!("\n=== Minimal serde example (v1.0.100) ===");
    ultra_simple_serde::main();

    println!("\n=== Manual serde implementation example (v1.0.100) ===");
    manual_serde::main();

    println!("\n=== Function pointer example ===");
    fn_pointer_example::main();

    println!("\n=== Fn/FnMut/FnOnce example ===");
    fn_trait_example::xxmain();

    println!("\n=== Unsafe Test Example ===");
    unsafe_test::main();

    println!("\n=== Drop Example ===");
    trigger_scope_drop();
}

mod unsafe_test {
    /// Target unsafe function for public exposure analysis
    pub unsafe fn dangerous_operation(input: i32) -> i32 {
        // Just a dummy unsafe function
        println!("Executing unsafe operation with {}", input);
        input * 2
    }

    /// Safe wrapper around unsafe function (Encapsulation depth = 1)
    pub fn safe_wrapper(input: i32) -> i32 {
        println!("Safe wrapper called");
        unsafe { dangerous_operation(input) }
    }

    /// Another layer (Encapsulation depth = 2 from dangerous_operation)
    pub fn another_layer(input: i32) -> i32 {
        safe_wrapper(input + 1)
    }

    // --- Complex Scenarios for Public Exposure Analysis ---

    // 1. Recursive Unsafe Chain
    // recursive_unsafe calls itself or is called by safe_recursive_entry
    pub unsafe fn recursive_unsafe(n: i32) -> i32 {
        if n <= 0 {
            dangerous_operation(0) // Call another unsafe function
        } else {
            recursive_unsafe(n - 1) + 1
        }
    }

    pub fn safe_recursive_entry(n: i32) -> i32 {
        unsafe { recursive_unsafe(n) }
    }

    // 2. Trait Safety Violation Simulation
    // A safe trait implemented unsafely (common pattern where implementation uses unsafe internals)
    pub trait SafeTrait {
        fn do_something(&self);
    }

    pub struct UnsafeImpl;

    impl SafeTrait for UnsafeImpl {
        fn do_something(&self) {
            unsafe {
                let _ = dangerous_operation(999);
            }
        }
    }

    pub fn use_trait_object(obj: &dyn SafeTrait) {
        obj.do_something();
    }

    pub fn main() {
        println!("Unsafe test main");
        let res = another_layer(42);
        println!("Result: {}", res);

        println!("Recursive test:");
        safe_recursive_entry(3);

        println!("Trait test:");
        let imp = UnsafeImpl;
        use_trait_object(&imp);
    }
}
