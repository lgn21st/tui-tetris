# 改进实施检查清单

基于 [IMPROVEMENT-PLAN.md](./IMPROVEMENT-PLAN.md) 的具体实施跟踪。

---

## 📊 实施前基线

**代码统计**:
- 总行数: 4,223 行 Rust
- 测试数: 114 个
- 文档行数: 1,125 行
- 提交数: 7 次

**性能基线** (待测量):
- [ ] 渲染时间: ___ ms
- [ ] 消行时间: ___ μs
- [ ] 分配/帧: ___ 次

---

## Phase 1: 游戏平衡修复 ⏳

**目标**: 达到 95%+ Guideline 兼容性
**时间**: 1-2 天

### P0 - 关键修复

- [ ] **1.1 修改 Lock Delay 500ms**
  - 文件: `src/types.rs:13`
  - 改动: `450` → `500`
  - 测试: 更新计时器测试

- [ ] **1.2 修复 B2B 奖励计算**
  - 文件: `src/core/scoring.rs:71`
  - 改动: `(base_score + combo_score) / 2`
  - 测试: 添加 B2B + Combo 测试用例

### P1 - 重要修复

- [ ] **1.3 调整 Soft Drop 20x**
  - 文件: `src/types.rs:11`
  - 改动: `10` → `20`
  - 验证: 下落速度符合预期

- [ ] **1.4 调整 DAS/ARR 常量**
  - 文件: `src/types.rs:19-20`
  - 改动: `150/50` → `167/33`
  - 文档: 更新规则说明

- [ ] **1.5 添加 ARE 框架**
  - 文件: `src/core/game_state.rs`
  - 添加: `are_timer_ms: u32` 字段
  - 添加: ARE 状态处理逻辑
  - 常量: `ARE_MS: u32 = 0`

### Phase 1 验收

- [ ] 所有计时参数测试通过
- [ ] B2B + Combo 计分测试通过
- [ ] 游戏手感验证正常
- [ ] `cargo test` 全通过

---

## Phase 2: 性能优化 ⏳

**目标**: 零分配热路径，<1ms 渲染
**时间**: 2-3 天
**依赖**: `arrayvec`, `criterion`

### 2.1 Board 重构

- [ ] **2.1.1 扁平化数组存储**
  - 文件: `src/core/board.rs`
  - 改动:
    ```rust
    // 从
    cells: Vec<Vec<Cell>>
    // 到
    cells: [Cell; (BOARD_WIDTH * BOARD_HEIGHT) as usize]
    ```
  - 重写: `get()`, `set()`, `clear_full_rows()`
  - 测试: 所有 16 个 board 测试仍需通过

- [ ] **2.1.2 零分配消行**
  - 添加依赖: `arrayvec = "0.7"`
  - 改动: `Vec<usize>` → `ArrayVec<usize, 4>`
  - 优化: 使用 `mem::swap` 代替 `clone`

### 2.2 增量渲染

- [ ] **2.2.1 创建 IncrementalRenderer**
  - 新文件: `src/ui/incremental.rs`
  - 实现:
    ```rust
    pub struct IncrementalRenderer {
        last_board: Board,
        last_active: Option<Tetromino>,
        changed_cells: Vec<(u8, u8)>,
    }
    ```
  - 方法: `diff()`, `render_cell()`, `render()`

- [ ] **2.2.2 集成到主循环**
  - 文件: `src/main.rs`
  - 替换: `BoardWidget` → `IncrementalRenderer`
  - 验证: 视觉输出完全一致

### 2.3 GameState 封装

- [ ] **2.3.1 私有字段**
  - 文件: `src/core/game_state.rs:56-80`
  - 改动: `pub` → `pub(crate)` 或完全私有
  - 字段: `board`, `score`, `level`, `lines`, etc.

- [ ] **2.3.2 添加 Getter 方法**
  - 实现:
    ```rust
    impl GameState {
        pub fn score(&self) -> u32 { self.score }
        pub fn level(&self) -> u32 { self.level }
        pub fn board(&self) -> &Board { &self.board }
        // ...
    }
    ```

- [ ] **2.3.3 更新调用者**
  - 文件: `src/main.rs`, `src/ui/widgets.rs`
  - 改动: `state.score` → `state.score()`

### 2.4 基准测试

- [ ] **2.4.1 添加 Criterion**
  - 添加依赖: `criterion = "0.5"`
  - 创建: `benches/game_logic.rs`
  - 测试:
    - `tick_16ms` - 游戏 tick 性能
    - `clear_4_lines` - 消行性能
    - `render_board` - 渲染性能

- [ ] **2.4.2 运行基准**
  - 命令: `cargo bench`
  - 目标: 
    - tick < 100μs
    - clear < 10μs
    - render < 1ms

### Phase 2 验收

- [ ] `cargo bench` 所有测试通过性能目标
- [ ] `heaptrack` 显示零分配/帧
- [ ] 1 小时连续运行测试通过
- [ ] 所有现有测试仍通过

---

## Phase 3: DAS/ARR 实现 ⏳

**目标**: 完整的按键自动重复系统
**时间**: 1-2 天

### 3.1 InputHandler 设计

- [ ] **3.1.1 创建 InputHandler 结构**
  - 新文件: `src/ui/input_handler.rs`
  - 结构:
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
    ```

- [ ] **3.1.2 实现 DAS 逻辑**
  - 方法: `update(elapsed_ms: u32) -> Vec<GameAction>`
  - 逻辑:
    1. 检测按键按下 → 立即触发一次动作
    2. 持续按住 → 启动 DAS 计时器
    3. DAS 达到 167ms → 开始 ARR
    4. ARR 每 33ms 触发一次动作

- [ ] **3.1.3 处理按键释放**
  - 方法: `handle_key_release(key: KeyCode)`
  - 重置: 对应方向的 DAS/ARR 计时器

### 3.2 集成到主循环

- [ ] **3.2.1 修改 main.rs**
  - 添加: `let mut input_handler = InputHandler::new();`
  - 修改事件循环:
    ```rust
    if let Event::Key(key) = event::read()? {
        match key.kind {
            KeyEventKind::Press => input_handler.handle_key_press(key.code),
            KeyEventKind::Release => input_handler.handle_key_release(key.code),
            _ => {}
        }
    }
    ```
  - 添加: `let auto_actions = input_handler.update(TICK_MS);`
  - 应用: `for action in auto_actions { game_state.apply_action(action); }`

### 3.3 测试

- [ ] **3.3.1 DAS 测试**
  - 测试: 按住左键 167ms 后自动重复
  - 测试: 松开后计时器重置

- [ ] **3.3.2 ARR 测试**
  - 测试: DAS 触发后每 33ms 一次移动
  - 测试: 快速交替按左右键不会冲突

### Phase 3 验收

- [ ] 长按方向键能持续移动
- [ ] DAS/ARR 计时准确
- [ ] 手感接近官方 Tetris
- [ ] 新增测试 5-10 个

---

## Phase 4: Adapter TCP Server ⏳

**目标**: 完整的 AI 控制支持
**时间**: 3-5 天
**依赖**: `tokio`

### 4.1 TCP Server 基础

- [ ] **4.1.1 创建 Server 结构**
  - 新文件: `src/adapter/server.rs`
  - 结构:
    ```rust
    pub struct TcpServer {
        listener: TcpListener,
        clients: HashMap<ClientId, Client>,
        controller: Option<ClientId>,
        game_state: Arc<Mutex<GameState>>,
    }
    ```

- [ ] **4.1.2 实现连接管理**
  - 方法: `accept_connections()` - 异步接受新连接
  - 方法: `handle_client()` - 客户端消息处理
  - 方法: `remove_client()` - 清理断开连接

### 4.2 Controller 管理

- [ ] **4.2.1 实现 Controller 模式**
  - 规则: 第一个发送 `hello` 的客户端成为 controller
  - 规则: 后续连接为 observer（只接收观测）
  - 规则: controller 断开时，最早 observer 晋升为 controller

- [ ] **4.2.2 权限验证**
  - 方法: `can_execute_command(client_id: ClientId) -> bool`
  - 错误: 非 controller 发送命令返回 `not_controller` 错误

### 4.3 消息处理

- [ ] **4.3.1 解析消息**
  - 使用: `adapter::protocol::parse_message()`
  - 处理: `hello`, `command`, `control` 消息类型

- [ ] **4.3.2 执行命令**
  - Action 模式: 解析 `moveLeft`, `rotateCw` 等 → `GameAction`
  - Place 模式: 解析 `(x, rotation, useHold)` → 计算动作序列

- [ ] **4.3.3 发送观测**
  - 频率: 每 tick 或节流（可配置）
  - 内容: `ObservationMessage` (board, active, score, timers)
  - 序列化: `serde_json::to_string()`

### 4.4 集成

- [ ] **4.4.1 添加 Server 启动**
  - 文件: `src/main.rs`
  - 代码:
    ```rust
    #[tokio::main]
    async fn main() {
        // ... 现有初始化
        let server = TcpServer::new("127.0.0.1:7777").await?;
        tokio::spawn(server.run());
        // ... 游戏循环
    }
    ```

- [ ] **4.4.2 共享状态**
  - 使用: `Arc<Mutex<GameState>>` 或 `Arc<RwLock<GameState>>`
  - 注意: 最小化锁持有时间

### 4.5 测试

- [ ] **4.5.1 单元测试**
  - 测试: 客户端连接/断开
  - 测试: Controller 晋升逻辑
  - 测试: 命令解析

- [ ] **4.5.2 集成测试**
  - 测试: 与 swiftui-tetris AI 客户端兼容
  - 测试: 高并发连接
  - 测试: 长时间运行稳定性

### Phase 4 验收

- [ ] AI 客户端可以连接并控制游戏
- [ ] 与 swiftui-tetris 100% 协议兼容
- [ ] 支持 10+ 并发 observer
- [ ] 零内存泄漏（24 小时测试）

---

## Phase 5: 打磨 ⏳

**目标**: 生产级品质
**时间**: 2-3 天

### 5.1 音效

- [ ] **5.1.1 添加 rodio 依赖**
  - `rodio = "0.17"`
  - 可选功能（feature flag）

- [ ] **5.1.2 实现音效系统**
  - 事件: Move, Rotate, Lock, Clear, T-Spin, GameOver
  - 文件: 从 assets/sfx/ 加载
  - 控制: 音量调节、静音

### 5.2 高分榜

- [ ] **5.2.1 本地存储**
  - 文件: `~/.tui-tetris/highscores.json`
  - 格式: `{name, score, lines, level, date}`
  - 保留: 前 10 名

- [ ] **5.2.2 游戏结束界面**
  - 显示: 本次得分、是否上榜
  - 输入: 玩家名字（如果上榜）

### 5.3 主题/配色

- [ ] **5.3.1 主题系统**
  - 配置: `~/.tui-tetris/config.toml`
  - 主题: Classic, Modern, Monochrome
  - 自定义: 每种方块颜色

### 5.4 统计

- [ ] **5.4.1 详细统计**
  - APM (Actions Per Minute)
  - PPS (Pieces Per Second)
  - 消行统计（1-4 行分布）
  - T-Spin 统计
  - 按键频率热力图

---

## 总体验收

### 功能验收

- [ ] 游戏可玩性达到官方 Tetris 95%+
- [ ] AI 客户端 100% 兼容
- [ ] 零崩溃、零内存泄漏
- [ ] 60 FPS 稳定运行

### 性能验收

- [ ] `cargo bench` 通过
  - [ ] tick < 100μs
  - [ ] clear < 10μs
  - [ ] render < 1ms
- [ ] `heaptrack` 显示 <1 分配/帧
- [ ] 1 小时压力测试通过

### 代码验收

- [ ] 测试覆盖 >90%
- [ ] Clippy 零警告
- [ ] 文档完整
- [ ] 代码审查通过

---

## 时间线总结

| Phase | 内容 | 天数 | 累计 |
|-------|------|------|------|
| 1 | 游戏平衡 | 1-2 | 2 |
| 2 | 性能优化 | 2-3 | 5 |
| 3 | DAS/ARR | 1-2 | 7 |
| 4 | TCP Server | 3-5 | 12 |
| 5 | 打磨 | 2-3 | 15 |

**总计**: 10-15 工作日（2-3 周）

---

## 立即开始

建议执行顺序:

1. **今天**: Phase 1（游戏平衡）
   - 修改 Lock Delay 500ms
   - 修复 B2B 奖励
   - 提交: `git commit -m "Fix: Lock delay 500ms, B2B bonus calculation"`

2. **明天**: Phase 2 开始（Board 重构）
   - 添加 `arrayvec` 依赖
   - 开始 Board 扁平化

3. **本周内**: Phase 3（DAS/ARR）
   - 实现 InputHandler
   - 测试手感

---

**状态**: 等待开始 ⏳
**负责人**: ___
**开始日期**: ___

*最后更新: 2026-02-02*