# TUI Tetris

高性能终端俄罗斯方块游戏，支持外部 AI 控制。

![Tetris](https://img.shields.io/badge/Rust-TUI-blue)
![Tests](https://img.shields.io/badge/Tests-passing-green)
![License](https://img.shields.io/badge/License-MIT-yellow)

## 🎮 快速开始

```bash
# 克隆仓库
git clone <repository-url>
cd tui-tetris

# 运行游戏
cargo run

# 运行测试
cargo test
```

## 🎯 特性

- ✅ **完整 Tetris 规则**: SRS 旋转系统、T-Spin 检测、B2B、连击
- ✅ **7-bag 随机生成器**: 确定性 RNG，支持种子
- ✅ **计分与 swiftui-tetris 对齐**: 经典行消、T-Spin 表分、B2B、Combo、Soft/Hard drop
- ✅ **完整游戏生命周期**: 开始、暂停、结束、重启
- ✅ **幽灵块 (Ghost Piece)**: 预览落点
- ✅ **Hold 功能**: 交换当前方块
- ✅ **AI 控制**: TCP 协议与 swiftui-tetris 100% 兼容
- ✅ **DAS/ARR 输入**: 专业级输入处理（延迟 167ms，重复 33ms）
- ✅ **终端“游戏渲染器”**: 自研 framebuffer + diff flush（不依赖 ratatui widgets）
- ✅ **完美宽高比**: 2:1 字符比例，方块呈正方形
- ⏳ **音效** (计划中)
- ⏳ **高分记录** (计划中)

## 🕹️ 控制

| 键位 | 动作 |
|------|------|
| `← →` / `A D` | 左右移动 |
| `↑` / `W` | 顺时针旋转 |
| `↓` / `S` | 软降 |
| `空格` | 硬降 |
| `C` | Hold |
| `P` | 暂停/恢复 |
| `R` | 重新开始 |
| `Q` / `Ctrl+C` | 退出 |

## 🏗️ 架构

```
┌─────────┐  ┌─────────┐  ┌─────────┐
│   UI    │  │  Core   │  │ Adapter │
├─────────┤  ├─────────┤  ├─────────┤
│ 输入    │  │ Board   │  │ Protocol│
│ 渲染    │←→│ Pieces  │←→│ (TCP)   │
│ 游戏循环│  │ RNG     │  │         │
│         │  │ Scoring │  │         │
│         │  │GameState│  │         │
└─────────┘  └─────────┘  └─────────┘
```

### 模块说明

- **Core**: 纯游戏逻辑（确定性、可测试）
- **Input**: crossterm 键盘输入 + DAS/ARR
- **Term Renderer**: 自研 framebuffer + diff flush（终端“游戏引擎”风格）
- **Adapter**: AI 控制协议 (JSON over TCP)

## 📁 项目结构

```
tui-tetris/
├── src/
│   ├── main.rs           # 游戏入口和主循环
│   ├── lib.rs            # 库导出
│   ├── types.rs          # 共享类型和常量
│   ├── input/            # 终端输入 (DAS/ARR)
│   │   ├── map.rs         # 键位映射
│   │   └── handler.rs     # DAS/ARR 输入处理
│   ├── core/             # 游戏逻辑 (无依赖)
│   │   ├── board.rs      # 10×20 棋盘
│   │   ├── pieces.rs     # 7 种方块 + SRS 旋转
│   │   ├── rng.rs        # 7-bag 随机生成器
│   │   ├── scoring.rs    # 计分规则
│   │   └── game_state.rs # 完整状态机
│   ├── term/             # 终端渲染 (framebuffer + diff flush)
│   │   ├── fb.rs          # FrameBuffer
│   │   ├── game_view.rs   # GameState -> FrameBuffer
│   │   └── renderer.rs    # FrameBuffer -> Terminal
│   └── adapter/          # AI 协议
│       ├── protocol.rs   # JSON 消息定义
│       └── mod.rs
│   └── engine/           # 可复用引擎辅助逻辑
│       └── place.rs      # place-mode 应用逻辑
├── tests/                # 集成测试
├── docs/                 # 文档
│   ├── rules-spec.md     # 游戏规则详情
│   └── adapter-protocol.md # AI 协议规范
│   ├── adapter_acceptance.md # AI 验收标准
│   ├── adapter-protocol.schema.json # 协议 JSON schema
│   ├── roadmap.md        # 当前维护路线图
│   └── feature-matrix.md # 功能矩阵
└── Cargo.toml
```

## 🧪 测试

```bash
# 运行所有测试
cargo test

# 运行特定模块测试
cargo test board
cargo test pieces
cargo test game_state

# 带覆盖率 (需要 cargo-tarpaulin)
cargo tarpaulin --out Html
```

**当前测试状态**: `cargo test` 通过 ✅

建议关注的测试套件:
- `cargo test --test adapter_acceptance_test`
- `cargo test --test adapter_closed_loop_test`
- `cargo test --test no_alloc_gate_test`

## 🎯 开发路线

### 已完成 ✅
- [x] 完整游戏可玩
- [x] 关键 acceptance/e2e/closed-loop 测试门槛
- [x] Core 层零外部依赖
- [x] Board 扁平化重构（1D 数组）
- [x] 完美宽高比渲染（2:1 字符比例）
- [x] DAS/ARR 输入处理（167ms/33ms）
- [x] TCP Server (tokio)
- [x] 控制器/观察者模式
- [x] 与 swiftui-tetris 100% 兼容
- [x] 完整 rustdoc 文档

### 计划中 ⏳
- [ ] 音效 (rodio)
- [ ] 高分记录持久化
- [ ] 主题/配色方案
- [ ] CI/CD (GitHub Actions)

## 📊 性能基准

主要性能门槛与后续优化计划见 `docs/roadmap.md`；基准测试用 `cargo bench`（见 `benches/`）。

## 📚 文档

### 技术规范
- [游戏规则](docs/rules-spec.md) - 完整 Tetris 规则 (SRS/计分/计时)
- [AI 协议](docs/adapter-protocol.md) - JSON 协议规范
- [AI Schema](docs/adapter-protocol.schema.json) - JSON schema（便于生成/校验）
- [AI 验收标准](docs/adapter_acceptance.md) - 协议/行为门槛与自测命令
- [Roadmap](docs/roadmap.md) - 当前维护的路线图
- [Feature Matrix](docs/feature-matrix.md) - 功能清单与状态
- [开发约定](AGENTS.md) - TDD 工作流程

## 🤝 兼容性

**AI 协议**: 与 swiftui-tetris 100% 兼容

环境变量:
- `TETRIS_AI_HOST` - 默认 127.0.0.1
- `TETRIS_AI_PORT` - 默认 7777
- `TETRIS_AI_DISABLED` - 禁用 Adapter

## 📝 贡献

遵循 TDD 开发流程:

1. 编写测试
2. 实现功能
3. 确保通过
4. 提交代码

## 📄 许可

MIT License - 详见 LICENSE 文件

---

**享受游戏！** 🎮

Made with ❤️ in Rust
