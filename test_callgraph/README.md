# Test Call Graph

This project is designed to test the Rust call graph generation tool, containing various complex Rust language features and function call relationships.

## Project Structure

This test project implements a simplified inventory management system with the following main components:

- **Product Interface(`Product` trait)**: Defines common product behaviors
- **Concrete Product Implementations(`Electronics`, `Clothing`)**: Implements the `Product` trait
- **Generic Data Storage(`DataStore<T>`)**: Generic data warehouse with specialized implementation for `Product` types
- **Inventory Manager(`InventoryManager`)**: Manages different types of products
- **Data Generator(`DataGenerator`)**: Generates random product data
- **Utility Functions**: Logging and other helper functions

## Test Cases

### 1. Trait Method Calls

The `Product` trait defines several methods, some with default implementations:

```rust
trait Product: Debug {
    fn name(&self) -> &str;
    fn price(&self) -> f64;
    fn category(&self) -> &str;
    fn discount(&self) -> f64 {
        0.0 // Default no discount
    }
    
    fn discounted_price(&self) -> f64 {
        self.price() * (1.0 - self.discount())
    }
}
```

Test Points:
- Trait default method call relationships
- Overridden trait methods
- Call chain: `discounted_price` -> `price` -> `discount`

Recommended Command:
```bash
call-cg --find-callers-of "Product::discounted_price"
```

### 2. Generic Methods and Specialized Implementations

```rust
// Generic implementation
impl<T> DataStore<T> {
    fn new(name: &str) -> Self { ... }
    fn add(&mut self, item: T) { ... }
    fn len(&self) -> usize { ... }
}

// Specialized implementation
impl<T: Product> DataStore<T> {
    fn total_value(&self) -> f64 { ... }
    fn total_discounted_value(&self) -> f64 { ... }
    fn calculate_value_with_strategy<F>(&self, strategy: F) -> f64 { ... }
    fn find_most_expensive(&self) -> Option<&T> { ... }
}
```

Test Points:
- Generic method instantiation
- Specialized implementation method calls
- Same method called with different generic parameters

Recommended Command:
```bash
call-cg --find-callers-of "DataStore::calculate_value_with_strategy"
```

### 3. Higher-Order Functions and Closures

```rust
fn calculate_value_with_strategy<F>(&self, strategy: F) -> f64 
where 
    F: Fn(&T) -> f64 
{
    let mut total = 0.0;
    for item in &self.items {
        let item_value = strategy(item);
        total += item_value;
        log_calculation(item.name(), item_value);
    }
    total
}
```

Test Points:
- Higher-order functions accepting closure parameters
- Closures calling other methods internally
- Closures capturing external variables

Recommended Command:
```bash
call-cg --find-callers-of "log_calculation"
```

### 4. Multi-Level Nested Calls

The `InventoryManager::generate_inventory_report` method calls multiple other methods, which in turn call more methods, forming a complex call tree.

```rust
fn generate_inventory_report(&self) -> String {
    // Calls DataStore::len
    // Calls DataStore::total_value
    // Calls DataStore::total_discounted_value
    // Calls DataStore::find_most_expensive
    // Calls self.append_sales_report
    // Each called method calls other methods
    ...
}
```

Test Points:
- Multi-level nested function calls
- Same function called through different paths

Recommended Command:
```bash
call-cg
cat ./target/callgraph.txt | grep "generate_inventory_report"
```

### 5. Trait Object Calls

```rust
fn generate_random_products<R: Rng>(rng: &mut R, count: usize) -> Vec<Rc<dyn Product>> {
    // Creates and uses trait objects
}
```

Test Points:
- Dynamic dispatch function calls
- Trait method calls through trait objects

Recommended Command:
```bash
call-cg --no-dedup
```

## Expected Test Results

When using this test project, you should observe the following types of call relationships:

1. **Direct Calls**: Such as `main` function directly calling `InventoryManager::create_example`

2. **Trait Calls**: Various method calls through the `Product` trait

3. **Generic Instantiations**: Same generic method instantiated with different types

4. **Closure Call Chains**: Closures as parameters and internally defined closures

5. **Dynamic Dispatch**: Method calls through trait objects

By analyzing these call relationships, you can verify that the call graph generation tool correctly handles various complex function call scenarios in Rust. 