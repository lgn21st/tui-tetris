# TUI-Tetris 全面评估与改进方案

**评估日期**: 2026-02-02  
**评估依据**: 
- 专家评审 (architecture-review.md)
- Tetris Guideline 标准
- 游戏性深度分析

---

## 一、当前状态评估

### 1.1 架构层面

| 维度 | 评分 | 说明 |
|------|------|------|
| 代码组织 | ⭐⭐⭐⭐⭐ | Core/Adapter/UI 分离清晰 |
| 测试覆盖 | ⭐⭐⭐⭐⭐ | 114 个测试，核心模块 90%+ |
| 性能 | ⭐⭐ | 热路径存在内存分配 |
| API 设计 | ⭐⭐⭐ | 封装性不足，接口不一致 |
| 错误处理 | ⭐⭐ | 缺乏结构化错误类型 |
| 文档 | ⭐⭐⭐⭐ | 新增专家级文档 |

### 1.2 游戏性层面

| 功能 | 状态 | 准确度 |
|------|------|--------|
| SRS 旋转 | ✅ | 100% - 踢墙表正确 |
| 7-bag RNG | ✅ | 100% - 符合 Guideline |
| 基础计分 | ✅ | 100% - 40/100/300/1200 |
| 锁定延迟 | ⚠️ | 90% - 应为 500ms 而非 450ms |
| T-Spin 检测 | ✅ | 95% - 3-corner 规则正确 |
| B2B 奖励 | ⚠️ | 80% - 未包含连击分数 |
| DAS/ARR | ❌ | 0% - 未实现 |
| ARE | ❌ | 0% - 缺失入场延迟 |
| Soft Drop | ⚠️ | 70% - 10x 应为 20-40x |

### 1.3 性能层面

**关键瓶颈**:

```
热点分析 (每帧):
├─ Board::clear_full_rows: 2 次堆分配 (Vec)
├─ UI 渲染: 200 个单元格全量重绘
├─ Vec<Vec<Cell>>: 双重指针跳转
└─ 总分配: 2-5 次/帧
```

**目标**:
- 零分配游戏循环
- <1ms 渲染时间
- <10μs 消行计算

---

## 二、与 Tetris Guideline 差异分析

### 2.1 计时参数

| 参数 | 当前值 | Guideline | 影响 |
|------|--------|-----------|------|
| Lock Delay | 450ms | **500ms** | 过高提升生存率，过低降低技术上限 |
| DAS | 未实现 | **167ms** | 影响移动手感 |
| ARR | 未实现 | **33ms** | 影响自动重复速度 |
| ARE | 缺失 | **0ms** | 消行后入场延迟 |

### 2.2 游戏机制

| 机制 | 当前 | Guideline | 严重程度 |
|------|------|-----------|----------|
| Soft Drop 重力 | 10x | **20-40x** | 影响速度控制 |
| B2B 奖励范围 | 仅消行分 | **含连击** | 影响高分策略 |
| T-Spin 判定 | 3-corner | 3-corner ✅ | 正确 |
| 锁定重置限制 | 15 次 | 15 次 ✅ | 正确 |

### 2.3 现代 Tetris 缺失功能

1. **DAS/ARR 系统** - 按键长按自动重复
2. **ARE (Entry Delay)** - 消行后入场延迟
3. ** sonic drop** - 立即降落但不锁定
4. **Initial Rotation System (IRS)** - 入场前预旋转
5. **Initial Hold System (IHS)** - 入场前预 Hold

---

## 三、关键问题详解

### 3.1 P0 - 游戏平衡性

#### 问题 1: Lock Delay 450ms vs 500ms

**影响**: 
- 50ms 差异看似微小，但对高级技巧（如 stalled techniques）至关重要
- 450ms 使游戏略难，500ms 是官方标准

**修复**:
```rust
pub const LOCK_DELAY_MS: u32 = 500;  // 原为 450
```

#### 问题 2: B2B 奖励未包含连击

**当前实现**:
```rust
let back_to_back_bonus = base_score / 2;  // 仅基础分
```

**Guideline 标准**:
- B2B 应用于**总分**（基础分 + 连击分）

**修复**:
```rust
let back_to_back_bonus = (base_score + combo_score) / 2;
```

#### 问题 3: DAS/ARR 缺失

**影响**: 
- 无法快速移动（如 Finesse 技巧）
- 手感与官方游戏差异大

**必须实现**。

### 3.2 P1 - 性能问题

#### 问题 1: Board 消行分配内存

**当前**:
```rust
pub fn clear_full_rows(&mut self) -> Vec<usize> {
    let mut cleared_rows = Vec::new();  // 分配 1
    // ...
    cleared_rows.reverse();             // 可能分配 2
    cleared_rows
}
```

**修复**:
```rust
use arrayvec::ArrayVec;

pub fn clear_full_rows(&mut self) -> ArrayVec<usize, 4> {
    let mut cleared_rows = ArrayVec::new();  // 栈分配
    // ...
    cleared_rows
}
```

#### 问题 2: Vec<Vec<Cell>> 双重间接

**当前**: 200 个指针跳转，缓存不友好

**修复**:
```rust
// 一维数组
pub struct Board {
    cells: [Cell; (BOARD_WIDTH * BOARD_HEIGHT) as usize],
}

fn index(x: i8, y: i8) -> usize {
    (y as usize) * BOARD_WIDTH as usize + (x as usize)
}
```

#### 问题 3: UI 全量重绘

**当前**: 每帧渲染所有 200 个单元格

**修复**: 增量渲染
```rust
pub struct IncrementalRenderer {
    last_board: Board,
    last_active: Option<Tetromino>,
}

impl IncrementalRenderer {
    pub fn render(&mut self, state: &GameState, buf: &mut Buffer) {
        // 仅渲染变化的单元格
        for (x, y) in state.board.diff(&self.last_board) {
            self.render_cell(buf, x, y, state.board.get(x, y));
        }
        // ...
    }
}
```

### 3.3 P2 - 完善性

#### Soft Drop 重力

**当前**: 10x 重力（间隔为正常的 1/10）

**Guideline**: 20-40x（通常固定为约 50ms 间隔）

**修复**:
```rust
pub const SOFT_DROP_MULTIPLIER: u32 = 20;  // 或实现为固定间隔
```

#### ARE 实现

即使为 0ms，也应显式实现以支持未来调整：

```rust
pub struct GameState {
    // ...
    are_timer_ms: u32,  // Entry delay timer
}
```

---

## 四、改进方案

### 4.1 阶段 1: 游戏平衡修复 (1-2 天)

**目标**: 达到 95%+ Guideline 兼容性

| 任务 | 文件 | 工作量 | 优先级 |
|------|------|--------|--------|
| 修改 Lock Delay 500ms | types.rs | 1 行 | P0 |
| 修复 B2B 奖励计算 | scoring.rs | 3 行 | P0 |
| 调整 Soft Drop 20x | types.rs | 1 行 | P1 |
| 调整 DAS/ARR 常量 | types.rs | 2 行 | P1 |
| 添加 ARE 框架 | game_state.rs | 20 行 | P1 |

**预计改动**: ~50 行代码

### 4.2 阶段 2: 性能优化 (2-3 天)

**目标**: 零分配热路径，<1ms 渲染

| 任务 | 文件 | 工作量 | 技术 |
|------|------|--------|------|
| Board 扁平化数组 | board.rs | 50 行 | 一维数组 |
| ArrayVec 消行 | game_state.rs | 5 行 | arrayvec crate |
| 增量渲染 | widgets.rs | 100 行 | Diff 算法 |
| GameState 封装 | game_state.rs | 100 行 | 私有字段 + getter |
| 基准测试 | benches/ | 50 行 | Criterion |

**依赖添加**:
```toml
[dependencies]
arrayvec = "0.7"

[dev-dependencies]
criterion = "0.5"
```

**预计改动**: ~300 行代码

### 4.3 阶段 3: DAS/ARR 实现 (1-2 天)

**目标**: 完整的按键自动重复系统

**设计**:
```rust
pub struct InputHandler {
    // 按键状态
    left_held: bool,
    right_held: bool,
    down_held: bool,
    
    // DAS 计时器
    left_das_timer: u32,
    right_das_timer: u32,
    
    // ARR 计时器
    left_arr_timer: u32,
    right_arr_timer: u32,
}

impl InputHandler {
    pub fn update(&mut self, elapsed_ms: u32) -> Vec<GameAction> {
        let mut actions = Vec::new();
        
        // DAS 逻辑
        if self.left_held {
            if self.left_das_timer >= DEFAULT_DAS_MS {
                // DAS 已触发，开始 ARR
                self.left_arr_timer += elapsed_ms;
                while self.left_arr_timer >= DEFAULT_ARR_MS {
                    actions.push(GameAction::MoveLeft);
                    self.left_arr_timer -= DEFAULT_ARR_MS;
                }
            } else {
                self.left_das_timer += elapsed_ms;
            }
        }
        // ... 右方向同理
        
        actions
    }
}
```

**集成到主循环**:
```rust
// main.rs
let input_handler = InputHandler::new();

loop {
    // 处理事件
    if event::poll(timeout)? {
        if let Event::Key(key) = event::read()? {
            input_handler.handle_key(key);
        }
    }
    
    // 获取 DAS/ARR 生成的动作
    let auto_actions = input_handler.update(TICK_MS);
    for action in auto_actions {
        game_state.apply_action(action);
    }
    
    // ... 现有游戏逻辑
}
```

**预计改动**: ~150 行代码

### 4.4 阶段 4: Adapter TCP Server (3-5 天)

**目标**: 完整的 AI 控制支持

**架构**:
```
┌──────────────┐
│  TcpServer   │
│  ├─ Listener │
│  ├─ Clients  │
│  └─ Router   │
└──────┬───────┘
       │
┌──────▼───────┐
│  Controller  │
│  ├─ 状态机   │
│  ├─ 权限管理 │
│  └─ 观察者   │
└──────┬───────┘
       │
┌──────▼───────┐
│  GameState   │
└──────────────┘
```

**关键组件**:
1. **TcpServer** - tokio 异步 TCP 监听
2. **ClientManager** - 连接生命周期管理
3. **Controller** - 控制权切换（第一个连接者为 controller）
4. **Observer** - 其他客户端只接收观测
5. **Throttler** - 观测频率节流

**预计改动**: ~500 行代码

### 4.5 阶段 5: 打磨 (2-3 天)

- 音效支持 (rodio)
- 高分榜 (本地文件)
- 主题/配色
- 设置界面
- 详细统计

---

## 五、实施路线图

```
Week 1                    Week 2                    Week 3
│                         │                         │
├── Phase 1: 游戏平衡 ────┼── Phase 2: 性能优化 ────┼── Phase 4: Adapter ─────┐
│                         │                         │                         │
│ ├─ Lock Delay 500ms     │ ├─ Board 扁平化         │ ├─ TCP Server           │
│ ├─ B2B 修复             │ ├─ ArrayVec 消行        │ ├─ Controller           │
│ ├─ Soft Drop 20x        │ ├─ 增量渲染             │ ├─ Protocol             │
│ └─ ARE 框架             │ └─ 基准测试             │ └─ 集成测试             │
│                         │                         │                         │
├── Phase 3: DAS/ARR ─────┤                         │                         │
│                         │                         │                         │
│ ├─ InputHandler         │                         │                         │
│ ├─ DAS 逻辑             │                         │                         │
│ └─ ARR 逻辑             │                         │                         │
│                         │                         │                         │
└─────────────────────────┴─────────────────────────┴── Phase 5: 打磨 ────────┘
                                                        │
                                                        ├─ 音效
                                                        ├─ 高分榜
                                                        └─ 主题
```

---

## 六、预期成果

### 6.1 游戏性

| 指标 | 当前 | 目标 |
|------|------|------|
| Guideline 兼容度 | 85% | **98%** |
| 玩家体验 | 良好 | **优秀** |
| 竞技可行性 | 低 | **中** |

### 6.2 技术

| 指标 | 当前 | 目标 |
|------|------|------|
| 测试覆盖 | 114 | **150+** |
| 分配/帧 | 2-5 | **0** |
| 渲染时间 | ~2ms | **<1ms** |
| AI 兼容性 | 0% | **100%** |

### 6.3 项目健康度

| 维度 | 当前 | 目标 |
|------|------|------|
| 代码质量 | B+ | **A** |
| 性能 | C+ | **A** |
| 可维护性 | A | **A** |
| 功能完整度 | 70% | **95%** |

---

## 七、风险评估

| 风险 | 概率 | 影响 | 缓解措施 |
|------|------|------|----------|
| Board 重构引入 bug | 中 | 高 | 完整回归测试 |
| DAS/ARR 手感不佳 | 中 | 中 | 可调节参数 |
| TCP Server 复杂度 | 高 | 中 | 先实现简化版 |
| 性能优化收益不足 | 低 | 低 | 基准测试验证 |

---

## 八、验收标准

### 8.1 功能验收

- [ ] 与 swiftui-tetris AI 客户端 100% 兼容
- [ ] 所有 Guideline 计时参数正确
- [ ] DAS/ARR 可配置且手感良好
- [ ] 零崩溃，所有边界情况处理

### 8.2 性能验收

- [ ] `cargo bench` 所有测试 <1ms
- [ ] 内存分析显示零分配/帧
- [ ] 1 小时连续运行无内存泄漏
- [ ] 60 FPS 稳定（<16ms/帧）

### 8.3 代码验收

- [ ] 新增代码 80%+ 测试覆盖
- [ ] 所有 Clippy 警告修复
- [ ] 文档完整，示例代码运行
- [ ] 代码审查通过

---

## 九、总结

**当前项目状态**: 优秀的 MVP，可玩且架构良好

**主要差距**:
1. 计时参数微调（Lock Delay 500ms）
2. DAS/ARR 系统缺失（影响手感）
3. 热路径内存分配（影响性能）
4. Adapter 未完成（影响 AI）

**改进后预期**:
- 游戏性达到 98% Guideline 兼容
- 性能达到零分配、<1ms 渲染
- 功能完整支持 AI 训练

**建议启动顺序**:
1. **立即**: Phase 1（游戏平衡）- 1 天
2. **本周**: Phase 3（DAS/ARR）- 2 天
3. **下周**: Phase 2（性能优化）- 3 天
4. **随后**: Phase 4（Adapter）- 5 天

---

**评估完成**  
*详见 architecture-review.md 和本改进方案*