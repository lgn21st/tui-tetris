# TUI Tetris - 专家级架构评审

**评审日期**: 2026-02-02  
**评审者**: Rust + TUI 专家  
**项目状态**: MVP 完成，需要性能和架构优化

---

## 1. 整体架构评估

### 1.1 架构图（当前）

```
┌─────────────────────────────────────────────────────────┐
│                         UI 层                            │
│  ┌──────────────┐ ┌──────────────┐ ┌──────────────────┐ │
│  │   主循环     │ │  输入处理    │ │   渲染 (60fps)   │ │
│  │  main.rs     │ │ input.rs     │ │  widgets.rs      │ │
│  └──────┬───────┘ └──────┬───────┘ └────────┬─────────┘ │
└─────────┼────────────────┼──────────────────┼───────────┘
          │                │                  │
┌─────────┼────────────────┼──────────────────┼───────────┐
│         │           Core 层                  │           │
│  ┌──────▼──────┐ ┌──────▼──────┐ ┌──────────▼────────┐ │
│  │  GameState  │ │    Board    │ │  Pieces/RNG/Score │ │
│  │  (114 测试) │ │  (16 测试)  │ │   (45 测试)       │ │
│  └─────────────┘ └─────────────┘ └───────────────────┘ │
└─────────────────────────────────────────────────────────┘
          │
          ▼
┌─────────────────────────────────┐
│     Adapter 层                   │
│  ┌────────────────────────────┐ │
│  │  Protocol (JSON) ✓        │ │
│  │  TCP Server (缺失)         │ │
│  └────────────────────────────┘ │
└─────────────────────────────────┘
```

### 1.2 架构得分

| 维度 | 评分 | 说明 |
|------|------|------|
| 模块分离 | ⭐⭐⭐⭐⭐ | Core/Adapter/UI 边界清晰 |
| 性能 | ⭐⭐ | 热路径有严重分配问题 |
| 可测试性 | ⭐⭐⭐⭐⭐ | 114 个单元测试 |
| API 设计 | ⭐⭐⭐ | 封装性不足，API 不一致 |
| 错误处理 | ⭐⭐ | 缺乏结构化错误类型 |
| 文档 | ⭐⭐⭐ | 基本文档，缺乏深层说明 |

---

## 2. 关键问题与改进

### 2.1 P0 - 关键性能问题

#### 问题 1: Board::clear_full_rows 每次消行都分配内存

**当前实现** (game_state.rs:98-124):
```rust
pub fn clear_full_rows(&mut self) -> Vec<usize> {
    let mut cleared_rows = Vec::new();  // ❌ 每次调用都分配！
    // ...
    cleared_rows.reverse();             // ❌ 可能重新分配！
    cleared_rows
}
```

**影响**: 每锁定一个方块就分配两次（最坏情况）

**修复方案**:
```rust
use arrayvec::ArrayVec;  // 栈上分配，无堆分配

pub fn clear_full_rows(&mut self) -> ArrayVec<usize, 4> {
    let mut cleared_rows = ArrayVec::new();  // ✅ 栈上，最多 4 行
    // ...
    // 无需 reverse - ArrayVec 支持从头部插入
    cleared_rows
}
```

#### 问题 2: Board 使用 Vec<Vec<Cell>> 双重间接寻址（已修复）

**当前**:
```rust
cells: Vec<Vec<Cell>>  // 200 个指针跳转，缓存不友好
```

**修复**:
```rust
cells: [Cell; (BOARD_WIDTH * BOARD_HEIGHT) as usize]  // ✅ 连续内存

pub fn get(&self, x: i8, y: i8) -> Option<Cell> {
    if x < 0 || x >= BOARD_WIDTH as i8 || y < 0 || y >= BOARD_HEIGHT as i8 {
        return None;
    }
    let idx = (y as usize) * (BOARD_WIDTH as usize) + (x as usize);
    Some(self.cells[idx])
}
```

#### 问题 3: UI 每帧渲染所有单元格（已修复）

**当前** (widgets.rs): 即使无变化也重绘 200 个单元格

**修复**: 实现增量渲染
```rust
pub struct BoardRenderer {
    last_board: Board,
    last_active: Option<Tetromino>,
}

impl BoardRenderer {
    pub fn render(&mut self, state: &GameState, buf: &mut Buffer) {
        // 只渲染变化的单元格
        for (x, y) in state.board.diff(&self.last_board) {
            render_cell(buf, x, y, state.board.get(x, y));
        }
        // ...
        self.last_board = state.board.clone();
    }
}
```

### 2.2 P1 - 架构设计问题

#### 问题 1: GameState 所有字段都是 pub

**风险**: 外部代码可直接修改状态，破坏不变式

**修复**:
```rust
pub struct GameState {
    board: Board,           // ✅ 私有
    score: u32,             // ✅ 私有
    level: u32,             // ✅ 私有
    // ...
}

impl GameState {
    pub fn score(&self) -> u32 { self.score }
    pub fn level(&self) -> u32 { self.level }
    // 只允许通过方法修改，保持状态一致性
}
```

#### 问题 2: 不一致的 API 返回类型

| 方法 | 返回类型 | 问题 |
|------|----------|------|
| try_move | bool | 简单 |
| try_rotate | bool | 简单 |
| hard_drop | u32 | 返回分数 |
| hold | bool | 简单 |
| apply_action | bool | 简单 |

**修复**: 统一返回 Result 类型
```rust
pub enum ActionResult {
    Success,
    Blocked,           // 移动/旋转被阻挡
    InvalidState,      // 游戏暂停/结束
    NothingHappened,   // 空操作
    Score(u32),        // 硬降得分
}

pub fn try_move(&mut self, dx: i8, dy: i8) -> ActionResult;
```

### 2.3 P2 - 代码质量问题

#### 问题: types.rs 中的 case-insensitive 解析分配 String

```rust
// 当前 - 分配内存
match s.to_lowercase().as_str() { ... }

// 修复 - 零分配
match s {
    s if s.eq_ignore_ascii_case("i") => Some(PieceKind::I),
    s if s.eq_ignore_ascii_case("o") => Some(PieceKind::O),
    // ...
}
```

---

## 3. 理想的架构设计

### 3.1 目标架构图

```
┌──────────────────────────────────────────────────────────────┐
│                          UI 层                                │
│  ┌──────────────┐ ┌──────────────┐ ┌──────────────────────┐ │
│  │   App        │ │  Event Loop  │ │   Renderer           │ │
│  │  (协调)      │ │ (16ms tick)  │ │  (增量渲染)          │ │
│  └──────┬───────┘ └──────┬───────┘ └────────┬─────────────┘ │
└─────────┼────────────────┼──────────────────┼───────────────┘
          │                │                  │
          ▼                ▼                  ▼
┌─────────────────────────────────────────────────────────────┐
│                    Core 层 (无外部依赖)                      │
│                                                              │
│  ┌──────────────┐ ┌──────────────┐ ┌──────────────────────┐│
│  │  GameState   │ │    Board     │ │  Tetromino/Pieces   ││
│  │  ├─ 状态管理 │ │  ├─ 扁平数组 │ │  ├─ 预计算形状      ││
│  │  ├─ 计时器   │ │  ├─ 无分配   │ │  ├─ 静态踢墙表      ││
│  │  ├─ 动作应用 │ │  └─ 位运算？ │ │  └─ SRS 旋转        ││
│  │  └─ 快照     │ │              │ │                     ││
│  └──────────────┘ └──────────────┘ └──────────────────────┘│
│                                                              │
│  ┌──────────────┐ ┌──────────────┐ ┌──────────────────────┐│
│  │     RNG      │ │   Scoring    │ │  Rules/T-Spin       ││
│  │  ├─ 确定性   │ │  ├─ 零分配   │ │  ├─ 预计算表        ││
│  │  └─ 7-bag    │ │  └─ 查表     │ │  └─ 规则验证        ││
│  └──────────────┘ └──────────────┘ └──────────────────────┘│
└─────────────────────────────────────────────────────────────┘
          ▲
          │
┌─────────┼──────────────────────────────────────────────────┐
│         │              Adapter 层                           │
│  ┌──────┴──────┐ ┌──────────────┐ ┌──────────────────────┐│
│  │   Server    │ │   Protocol   │ │    Controller        ││
│  │  (TCP)      │ │  ├─ serde    │ │   ├─ 状态机          ││
│  │  ├─ tokio   │ │  └─ 验证     │ │   ├─ 观察者模式      ││
│  │  └─ 并发    │ │              │ │   └─ 节流            ││
│  └─────────────┘ └──────────────┘ └──────────────────────┘│
└─────────────────────────────────────────────────────────────┘
```

### 3.2 关键设计原则

1. **零分配热路径**: tick(), render(), apply_action() 不分配内存
2. **不可变状态快照**: GameState 可导出不可变快照供 Adapter/UI 读取
3. **动作命令模式**: 所有修改通过 Action 枚举，便于回放和 AI 训练
4. **确定性**: 相同种子产生相同游戏序列（已✓）
5. **扁平化存储**: Board 使用一维数组，Piece 使用静态表

---

## 4. 与 swiftui-tetris 对比

| 特性 | swiftui-tetris | tui-tetris (当前) | tui-tetris (目标) |
|------|----------------|-------------------|-------------------|
| Core 语言 | Swift | Rust ✅ | Rust ✅ |
| UI 技术 | SwiftUI+SpriteKit | crossterm + custom framebuffer ✅ | crossterm + custom framebuffer ✅ |
| AI 协议 | TCP+JSON | TCP+JSON ✅ | TCP+JSON ✅ |
| 渲染性能 | 60fps GPU | 60fps CPU (diff flush) ✅ | <1ms/frame (目标) |
| 内存分配 | 预分配节点 | core 热路径零分配 ✅ | end-to-end 零分配（目标） |
| 测试覆盖 | >90% | acceptance/e2e + core 单测 ✅ | 覆盖率门槛（目标） |
| DAS/ARR | ✓ | ✓ | ✓ |
| 音效 | AVAudioEngine | ✗ | rodio (可选) |

---

## 5. 实施路线图

### Phase 1: 关键修复 (1-2 天)
- [x] 修复 Board 内存分配 (ArrayVec + 扁平数组)
- [x] 修复 T-spin 检测 (corner + last-rotate)
- [x] 移除未使用依赖 (按当前 Cargo.lock 为准)
- [x] 结构化协议错误码 + e2e/acceptance tests

### Phase 2: 性能优化 (2-3 天)
- [x] 实现增量渲染 (framebuffer diff flush)
- [x] 添加 DAS/ARR 输入处理（支持无 release event 终端）
- [ ] GameState API 封装化（进行中）
- [ ] 基准测试框架（待实现；见 PERFORMANCE.md）

### Phase 3: Adapter 完成 (3-5 天)
- [x] TCP Server (tokio)
- [x] Controller/Observer 管理
- [x] 消息节流 / backpressure
- [x] AI 客户端兼容测试（e2e + acceptance + closed-loop）

### Phase 4: 打磨 (2-3 天)
- [ ] 音效支持 (rodio)
- [ ] 高分榜
- [ ] 主题/颜色配置
- [ ] 文档完善

---

## 6. 推荐 Crate

### 性能
- `arrayvec` - 栈上固定容量数组，无分配
- `smallvec` - 小容量栈优化
- `criterion` - 基准测试

### Terminal
- `crossterm` - 已使用 ✅
- 自研 framebuffer + diff flush - 已使用 ✅

### 异步
- `tokio` - 已使用 ✅
- `tokio-stream` - 流处理

### 音频
- `rodio` - 跨平台音频
- `cpal` - 底层音频

---

## 7. 结论

**当前状态**: 游戏可玩，Core 层稳固，但性能和架构有待提升。

**优势**:
- 清晰的模块边界
- 全面的测试覆盖
- 正确的 Tetris 规则实现

**劣势**:
- 热路径分配过多
- Adapter 协议/测试已落地；持续维护兼容性
- API 封装不足

**建议**:
1. **短期**: 修复 Board 分配问题和 error handling
2. **中期**: 完成 Adapter，实现真正的 AI 兼容
3. **长期**: 增量渲染，音效，打磨 UX

这是一个**solid MVP**，但距离生产级的 AI 训练平台还有差距。优先修复 P0/P1 问题后，项目将达到 production-ready 状态。

---

**评审完成**  
*全面文档见 README.md, PERFORMANCE.md, ARCHITECTURE.md*
