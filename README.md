# 额度火花 QuotaSpark

一个 Windows 托盘小工具，用于**定时激活 Coding Plan / Token Plan 的五小时额度窗口**。

很多 Coding Plan（智谱 GLM、Kimi For Coding 等）的额度窗口从"第一次请求"开始计算，
滚动 5 小时后刷新。本工具在指定时刻向各供应商发送一条 `max_tokens=1` 的最小请求
（内容为 `"hi"`），把窗口"点火"到你想要的时间点——例如 05:30 / 10:30 / 15:30 三连发，
窗口就会首尾相接、全天可用。

## 功能

- **同步 cc-switch 配置**：一键读取 `~/.cc-switch/cc-switch.db`（只读），导入全部
  Claude 类供应商的名称 / 地址 / Key / 模型，之后 cc-switch 里改了 Key 再点一次同步即可更新；
  cc-switch 里配好的额度查询凭据（火山 AK/SK、智谱团队组织/项目 ID、ZenMux 用量端点）
  也会一并同步过来，本地手动填过的值不会被清空
- **手动添加供应商**：支持 Anthropic Messages 与 OpenAI Chat Completions 两种接口格式
- **按供应商设定定时**：每个供应商可设多个每日触发时间（HH:MM），可随时启停
- **多模型并发激活**：卡片单独"激活"，或托盘 / 顶栏"全部激活"
- **额度查询（与 cc-switch 全量对齐，12 家）**：
  - Coding Plan（窗口百分比）：Kimi For Coding、智谱 GLM 个人版（bigmodel.cn / z.ai）、
    智谱团队版（需组织/项目 ID）、MiniMax（中/英）、ZenMux（需填用量端点）、
    OpenCode Go、火山方舟 Agent/Coding Plan（需账号 AK/SK，自动探测两种套餐）
  - 按量余额：DeepSeek、StepFun、SiliconFlow（中/英）、OpenRouter、Novita AI
  - 官方 OAuth 订阅（Claude/ChatGPT/Gemini/Copilot/xAI 官方账号）不搬移：
    依赖 cc-switch 的 OAuth token 刷新链路，且对官方订阅做自动化风险最高
  - 不在支持清单里的供应商不显示查询按钮（避免点了必报错），激活不受影响
- **托盘常驻**：关闭窗口 = 隐藏到托盘，调度器持续运行；托盘菜单可"立即全部激活"和退出
- **日志**：激活 / 查询结果记录在界面底部与 `~/.cc-activator/logs/activator.log`

## 开发

```bash
npm install
npm run tauri dev     # 开发调试
npm run tauri build   # 打包安装程序
```

技术栈：Tauri 2 · Rust · React 18 · TypeScript。

## 数据与安全

- 配置（含 API Key）只保存在本机 `~/.cc-activator/config.json`，原子写入，不上传
- 对 cc-switch 数据库**只读不写**，删除本工具不影响 cc-switch
- 激活请求本身会计入套餐用量（一条 `hi`、1 个 token 的成本，可忽略不计）
- 应用重启后 10 分钟宽限内的定时不会重复发送（去重键持久化在配置里）

## 风险提醒

部分厂商的服务条款对"自动化客户端"有约定，请自行评估；官方订阅（Claude/OpenAI
官方账号）建议不要用于此类自动化。

## 自动更新

应用内置 Tauri 官方更新器（`tauri-plugin-updater`）：

1. **签名密钥**：私钥在 `~/.tauri/quotaspark.key`（公钥 `quotaspark.key.pub` 已写入
   `src-tauri/tauri.conf.json`）。**私钥丢失则无法再发布更新，务必备份**；私钥绝不入库（.gitignore 已排除 `*.key`）
2. **发新版本**：
   - 改版本号：`package.json`、`src-tauri/Cargo.toml`、`src-tauri/tauri.conf.json` 三处同步
   - 打包并签名（私钥通过环境变量传入）：
     ```bash
     TAURI_SIGNING_PRIVATE_KEY=$(cat ~/.tauri/quotaspark.key) \
     TAURI_SIGNING_PRIVATE_KEY_PASSWORD="" \
     npm run tauri build
     ```
   - 产物在 `src-tauri/target/release/bundle/`，安装包旁会有同名 `.sig` 签名文件
3. **发布到 GitHub**：在 Releases 上传安装包（建议先 zip 一层）+ `.sig`，并上传一份
   `latest.json`（格式如下），用户端「设置 → 检查更新」即可自动发现并一键安装：
   ```json
   {
     "version": "0.2.0",
     "notes": "更新说明",
     "pub_date": "2026-09-29T12:00:00Z",
     "platforms": {
       "windows-x86_64": {
         "signature": "（.sig 文件的内容）",
         "url": "https://github.com/<你的用户名>/quotaspark/releases/download/v0.2.0/QuotaSpark_0.2.0_x64-setup.zip"
       }
     }
   }
   ```
4. **更新源**：`tauri.conf.json → plugins.updater.endpoints` 目前是占位地址
   （`YOUR_GITHUB_USERNAME`），发布前替换成你自己的
   `https://github.com/<用户名>/quotaspark/releases/latest/download/latest.json`

## 测试

- Rust 单测：`cd src-tauri && cargo test`
- 前端类型检查：`npx tsc --noEmit`
- 端到端（真实驱动 UI 的增删改查/调度/主题/更新入口）：应用以
  `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS="--remote-debugging-port=9222" npm run tauri dev`
  启动后执行 `npm run e2e`
