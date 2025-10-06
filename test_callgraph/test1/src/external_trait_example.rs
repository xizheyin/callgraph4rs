// 外部crate trait动态调用示例
// 演示如何在动态分发中使用外部crate的trait

use std::fmt::{Debug, Display};
use std::collections::HashMap;

// 定义一个包装trait，用于动态调用外部crate的trait方法
trait DataProcessor: Debug + Send + Sync {
    fn process_data(&self) -> String;
    fn format_output(&self) -> String;
    fn get_type_name(&self) -> &'static str;
    fn calculate_hash(&self) -> u64;
}

// 实现DataProcessor的具体类型 - 数值处理器
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

    // 动态调用Display trait方法
    fn format_output(&self) -> String {
        format!("Value: {}, Multiplier: {}", self.value, self.multiplier)
    }

    // 动态调用std::hash相关功能
    fn calculate_hash(&self) -> u64 {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        
        let mut hasher = DefaultHasher::new();
        // 将f64转换为字节进行hash
        self.value.to_bits().hash(&mut hasher);
        self.multiplier.to_bits().hash(&mut hasher);
        hasher.finish()
    }

    fn get_type_name(&self) -> &'static str {
        std::any::type_name::<Self>()
    }
}

// 实现DataProcessor的具体类型 - 字符串处理器
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

    // 动态调用Display trait方法
    fn format_output(&self) -> String {
        format!("Text: '{}', Prefix: '{}'", self.text, self.prefix)
    }

    // 动态调用std::hash相关功能
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

// 使用Box<dyn DataProcessor>进行动态分派
fn process_with_dynamic_dispatch(processor: Box<dyn DataProcessor>) {
    println!("Processing with type: {}", processor.get_type_name());
    
    // 动态调用process_data方法
    let processed = processor.process_data();
    println!("Processed data: {}", processed);
    
    // 动态调用format_output方法
    let formatted = processor.format_output();
    println!("Formatted output: {}", formatted);
    
    // 动态调用calculate_hash方法
    let hash_value = processor.calculate_hash();
    println!("Hash value: {}", hash_value);
}

// 使用trait对象向量进行动态分派
fn demonstrate_trait_object_vector() {
    println!("\n=== Trait Object Vector Example ===");
    
    let processors: Vec<Box<dyn DataProcessor>> = vec![
        Box::new(NumberProcessor::new(10.5, 2.0)),
        Box::new(StringProcessor::new("Hello World".to_string(), "Message".to_string())),
        Box::new(NumberProcessor::new(42.0, 0.5)),
    ];
    
    for (i, processor) in processors.into_iter().enumerate() {
        println!("--- Processor {} ---", i + 1);
        // 每次循环都会进行动态分派
        println!("Type: {}", processor.get_type_name());
        println!("Data: {}", processor.process_data());
        println!("Format: {}", processor.format_output());
        println!("Hash: {}", processor.calculate_hash());
    }
}

// 使用闭包和trait对象的组合
fn functional_style_processing() {
    println!("\n=== Functional Style Processing ===");
    
    let processors: Vec<Box<dyn DataProcessor>> = vec![
        Box::new(NumberProcessor::new(5.0, 3.0)),
        Box::new(StringProcessor::new("Rust".to_string(), "Language".to_string())),
    ];
    
    // 使用迭代器和闭包进行函数式处理
    let results: Vec<_> = processors
        .into_iter()
        .map(|processor| {
            // 动态分派调用
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

// 演示HashMap中存储trait对象
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

// 主函数，演示标准库trait的动态调用
pub fn main() {
    println!("=== Standard Library Trait Dynamic Dispatch Example ===");
    
    // 创建不同类型的处理器
    let number_processor = Box::new(NumberProcessor::new(42.0, 1.5));
    let string_processor = Box::new(StringProcessor::new(
        "Dynamic Dispatch".to_string(),
        "Example".to_string(),
    ));
    
    // 单独处理每个处理器
    println!("\n--- Individual Processing ---");
    process_with_dynamic_dispatch(number_processor);
    process_with_dynamic_dispatch(string_processor);
    
    // 演示trait对象向量
    demonstrate_trait_object_vector();
    
    // 演示函数式风格
    functional_style_processing();
    
    // 演示trait对象存储
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
        assert_eq!(hash1, hash2); // 相同输入应该产生相同hash
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
            // 确保动态分派正常工作
        }
    }
}
