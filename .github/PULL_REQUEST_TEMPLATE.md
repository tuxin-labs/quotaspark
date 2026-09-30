<!-- 提交前请确认 CI 要求的检查已本地通过：npx tsc --noEmit；cd src-tauri && cargo fmt --check && cargo clippy -- -D warnings && cargo test -->

## 改动说明

<!-- 改了什么，为什么这么改 -->

## 关联 Issue

<!-- closes #123，没有就写"无" -->

## 测试方式

<!-- 你怎么验证的：新增/修改了哪些单测，e2e 是否通过，手动操作路径 -->

## 自检

- [ ] `npx tsc --noEmit` 零错误
- [ ] `cargo fmt` 与 `cargo clippy -- -D warnings` 通过
- [ ] `cargo test` 通过
- [ ] 涉及 UI 的改动跑过 `npm run e2e`
- [ ] 未引入新的网络请求（如有，已在关联 Issue 中讨论）
