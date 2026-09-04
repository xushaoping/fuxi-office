# Contributing

感谢你对 Fuxi-DMN 的关注。提交 Issue 或 Pull Request 前，请先说明复现步骤、
预期行为和实际行为；涉及决策规则时，请附上最小化的 DMN/DSL 示例，不要提交
真实客户数据、密钥或生产配置。

## 开发检查

```bash
rustup show
cargo fmt --all -- --check
cargo check --bin fuxi-dmn
cargo check --lib --target wasm32-unknown-unknown --features csr
cargo test --lib
```

提交前请确认没有 `.env`、构建产物、数据库备份或 IDE 文件，并保留与行为变更
对应的测试和文档更新。
