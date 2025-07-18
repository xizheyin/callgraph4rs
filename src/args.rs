use clap::Parser;
use rustc_hash::FxHashMap;
use serde::{Deserialize, Serialize};
use serde_json::{self, Value};
use std::path::PathBuf;

/// CG args
#[derive(Parser, Clone, Debug, Serialize, Deserialize)]
pub struct CGArgs {
    /// Show all functions
    #[clap(long = "show-all-funcs")]
    pub show_all_funcs: bool,

    /// Show all MIR
    #[clap(long = "show-all-mir")]
    pub show_all_mir: bool,

    /// Emit MIR
    #[clap(long = "emit-mir")]
    pub emit_mir: bool,

    /// Entry point of the program
    #[clap(long = "entry-point")]
    pub entry_point: Option<String>,

    /// Output directory
    #[arg(short, long)]
    pub output_dir: Option<PathBuf>,

    /// No deduplication for call sites
    /// When enabled, keeps all call sites for the same caller-callee pair
    #[arg(long, default_value_t = false)]
    pub no_dedup: bool,

    /// Find all callers of the specified function path
    /// When specified, will output all functions that directly or indirectly call this function
    #[arg(long, value_delimiter = ',')]
    pub find_callers: Vec<String>,

    /// Output the call graph as JSON format
    /// This provides machine-readable data for further processing
    #[arg(long, default_value_t = false)]
    pub json_output: bool,

    /// Do not include generic type arguments in function paths
    /// When enabled, function paths will not include generic type parameters
    #[arg(long, default_value_t = false)]
    pub without_args: bool,

    /// Output file for timing information
    /// When specified, will write detailed timing information to this file
    #[arg(long)]
    pub timer_output: Option<PathBuf>,

    /// Enable debug mode
    /// When enabled, will print debug information
    #[arg(long, default_value_t = false)]
    pub cg_debug: bool,

    /// Path to the manifest (Cargo.toml)
    /// When specified, will use this manifest path instead of auto-detecting
    #[arg(long)]
    pub manifest_path: Option<PathBuf>,

    /// Root path of the repository to analyze
    /// When specified, will use this as the base directory for manifest path
    #[arg(long)]
    pub root_path: Option<PathBuf>,
}

impl CGArgs {
    /// Convert CGArgs to a HashMap<String, String>
    pub fn to_hash_map(&self) -> FxHashMap<String, String> {
        // 将结构体序列化为 JSON 值
        let json_value = serde_json::to_value(self).unwrap();

        // 创建一个 HashMap 用于存储结果
        let mut map = FxHashMap::default();

        // 遍历 JSON 值的键值对
        if let Value::Object(obj) = json_value {
            for (key, value) in obj {
                // 将每个值转换为字符串
                let string_value = match value {
                    Value::String(s) => s,  // 直接使用字符串值
                    _ => value.to_string(), // 其他类型默认转换
                };
                map.insert(key, string_value);
            }
        }

        map
    }
}

#[derive(Parser, Clone, Debug, Serialize, Deserialize)]
#[clap(about = "This is a bug detector for Rust.")]
pub struct AllCliArgs {
    /// Arguments passed to cargo rust-analyzer
    #[arg(trailing_var_arg = true)]
    pub cargo_args: Vec<String>,

    #[command(flatten)]
    pub cg_args: CGArgs,
}
