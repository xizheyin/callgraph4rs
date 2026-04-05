// External crate trait dynamic dispatch example
// Demonstrates how to use traits from external crates with dynamic dispatch

use std::fmt::{Debug, Display};
use std::collections::HashMap;

// Wrapper trait used to dynamically call trait methods from external crates
trait DataProcessor: Debug + Send + Sync {
    fn process_data(&self) -> String;
    fn format_output(&self) -> String;
    fn get_type_name(&self) -> &'static str;
    fn calculate_hash(&self) -> u64;
}

// Concrete DataProcessor implementation for numeric values
#[derive(Debug, Clone)]
struct NumberProcessor {
    value: f64,
    multiplier: f64,
}

impl NumberProcessor {
    fn new(value: f64, multiplier: f64) -> Self {
        Self { value, multiplier }
    }
}

impl DataProcessor for NumberProcessor {
    fn process_data(&self) -> String {
        let result = self.value * self.multiplier;
        format!("NumberProcessor: {} * {} = {}", self.value, self.multiplier, result)
    }

    // Dynamically call Display-related formatting
    fn format_output(&self) -> String {
        format!("Value: {}, Multiplier: {}", self.value, self.multiplier)
    }

    // Dynamically use std::hash functionality
    fn calculate_hash(&self) -> u64 {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        
        let mut hasher = DefaultHasher::new();
        // Convert f64 values to bits before hashing
        self.value.to_bits().hash(&mut hasher);
        self.multiplier.to_bits().hash(&mut hasher);
        hasher.finish()
    }

    fn get_type_name(&self) -> &'static str {
        std::any::type_name::<Self>()
    }
}

// Concrete DataProcessor implementation for strings
#[derive(Debug, Clone)]
struct StringProcessor {
    text: String,
    prefix: String,
}

impl StringProcessor {
    fn new(text: String, prefix: String) -> Self {
        Self { text, prefix }
    }
}

impl DataProcessor for StringProcessor {
    fn process_data(&self) -> String {
        format!("{}: {}", self.prefix, self.text)
    }

    // Dynamically call Display-related formatting
    fn format_output(&self) -> String {
        format!("Text: '{}', Prefix: '{}'", self.text, self.prefix)
    }

    // Dynamically use std::hash functionality
    fn calculate_hash(&self) -> u64 {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        
        let mut hasher = DefaultHasher::new();
        self.text.hash(&mut hasher);
        self.prefix.hash(&mut hasher);
        hasher.finish()
    }

    fn get_type_name(&self) -> &'static str {
        std::any::type_name::<Self>()
    }
}

// Use Box<dyn DataProcessor> for dynamic dispatch
fn process_with_dynamic_dispatch(processor: Box<dyn DataProcessor>) {
    println!("Processing with type: {}", processor.get_type_name());
    
    // Dynamically call process_data
    let processed = processor.process_data();
    println!("Processed data: {}", processed);
    
    // Dynamically call format_output
    let formatted = processor.format_output();
    println!("Formatted output: {}", formatted);
    
    // Dynamically call calculate_hash
    let hash_value = processor.calculate_hash();
    println!("Hash value: {}", hash_value);
}

// Use a vector of trait objects for dynamic dispatch
fn demonstrate_trait_object_vector() {
    println!("\n=== Trait Object Vector Example ===");
    
    let processors: Vec<Box<dyn DataProcessor>> = vec![
        Box::new(NumberProcessor::new(10.5, 2.0)),
        Box::new(StringProcessor::new("Hello World".to_string(), "Message".to_string())),
        Box::new(NumberProcessor::new(42.0, 0.5)),
    ];
    
    for (i, processor) in processors.into_iter().enumerate() {
        println!("--- Processor {} ---", i + 1);
        // Dynamic dispatch happens in each iteration
        println!("Type: {}", processor.get_type_name());
        println!("Data: {}", processor.process_data());
        println!("Format: {}", processor.format_output());
        println!("Hash: {}", processor.calculate_hash());
    }
}

// Combine closures and trait objects
fn functional_style_processing() {
    println!("\n=== Functional Style Processing ===");
    
    let processors: Vec<Box<dyn DataProcessor>> = vec![
        Box::new(NumberProcessor::new(5.0, 3.0)),
        Box::new(StringProcessor::new("Rust".to_string(), "Language".to_string())),
    ];
    
    // Functional-style processing with iterators and closures
    let results: Vec<_> = processors
        .into_iter()
        .map(|processor| {
            // Dynamic dispatch call
            let data = processor.process_data();
            let formatted = processor.format_output();
            let hash = processor.calculate_hash();
            (data, formatted, hash)
        })
        .collect();
    
    for (i, (data, formatted, hash)) in results.iter().enumerate() {
        println!("Result {}: {}", i + 1, data);
        println!("  Formatted: {}", formatted);
        println!("  Hash: {}", hash);
    }
}

// Demonstrate storing trait objects in a HashMap
fn demonstrate_trait_object_storage() {
    println!("\n=== Trait Object Storage Example ===");
    
    let mut storage: HashMap<String, Box<dyn DataProcessor>> = HashMap::new();
    
    storage.insert("num1".to_string(), Box::new(NumberProcessor::new(100.0, 0.1)));
    storage.insert("str1".to_string(), Box::new(StringProcessor::new("Storage".to_string(), "Demo".to_string())));
    storage.insert("num2".to_string(), Box::new(NumberProcessor::new(25.0, 4.0)));
    
    for (key, processor) in storage.iter() {
        println!("Key: {}", key);
        println!("  Type: {}", processor.get_type_name());
        println!("  Data: {}", processor.process_data());
        println!("  Hash: {}", processor.calculate_hash());
    }
}

// Main entry demonstrating dynamic dispatch with standard library traits
pub fn main() {
    println!("=== Standard Library Trait Dynamic Dispatch Example ===");
    
    // Create processors of different types
    let number_processor = Box::new(NumberProcessor::new(42.0, 1.5));
    let string_processor = Box::new(StringProcessor::new(
        "Dynamic Dispatch".to_string(),
        "Example".to_string(),
    ));
    
    // Process each processor independently
    println!("\n--- Individual Processing ---");
    process_with_dynamic_dispatch(number_processor);
    process_with_dynamic_dispatch(string_processor);
    
    // Demonstrate a vector of trait objects
    demonstrate_trait_object_vector();
    
    // Demonstrate a functional style
    functional_style_processing();
    
    // Demonstrate trait object storage
    demonstrate_trait_object_storage();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_number_processor() {
        let processor = NumberProcessor::new(10.0, 2.0);
        let result = processor.process_data();
        assert!(result.contains("20"));
        
        let hash1 = processor.calculate_hash();
        let hash2 = processor.calculate_hash();
        assert_eq!(hash1, hash2); // Identical inputs should yield the same hash
    }

    #[test]
    fn test_string_processor() {
        let processor = StringProcessor::new("test".to_string(), "prefix".to_string());
        let result = processor.process_data();
        assert!(result.contains("test"));
        assert!(result.contains("prefix"));
        
        let formatted = processor.format_output();
        assert!(formatted.contains("test"));
        assert!(formatted.contains("prefix"));
    }

    #[test]
    fn test_dynamic_dispatch() {
        let processors: Vec<Box<dyn DataProcessor>> = vec![
            Box::new(NumberProcessor::new(5.0, 2.0)),
            Box::new(StringProcessor::new("hello".to_string(), "world".to_string())),
        ];
        
        assert_eq!(processors.len(), 2);
        
        for processor in processors {
            let _result = processor.process_data();
            let _hash = processor.calculate_hash();
            // Ensure dynamic dispatch works correctly
        }
    }
}
