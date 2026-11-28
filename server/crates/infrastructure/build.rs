fn main() {
    // 让已有的本地构建也能发现新 SQL 文件并重新嵌入迁移清单。
    println!("cargo::rerun-if-changed=../../migrations");
}
