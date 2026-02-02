# TUI-Tetris 最终项目评估与改进方案总结

NOTE (2026-02-02): This document is retained for history.
For the maintained, current view use:
- `docs/feature-matrix.md`
- `docs/roadmap.md`

## 状态更新（当前实现）

- Core 计时：`LOCK_DELAY_MS=500`、`SOFT_DROP_MULTIPLIER=20`、固定步长 `TICK_MS=16`
- 规则：7-bag、SRS、T-Spin、combo、B2B 已实现且有测试覆盖
- Board：扁平数组 + `ArrayVec` 消行（热路径零分配）
- 渲染：crossterm + 自研 framebuffer + diff flush
- 输入：DAS/ARR 已实现（含无 key-release 终端的超时释放）
- Adapter：协议兼容 + e2e/acceptance/closed-loop tests

**评估日期**: 2026-02-02  
**评估依据**: 代码审查、架构评审、Tetris Guideline 标准

---

## 一、当前项目统计（历史快照）

### 1.1 代码基线

```
该段统计为历史快照；当前请以 `git log` / `cargo test` / `cargo clippy` 输出为准。
```

### 1.2 代码分布

| 模块 | 文件数 | 代码行数 | 测试数 | 覆盖率 |
|------|--------|----------|--------|--------|
| Core | 6 | ~2,500 | 91 | ~90% |
| Adapter | 2 | ~500 | 0 | 0% |
| UI | 4 | ~663 | 23 | ~70% |

### 1.3 架构评分

| 维度 | 评分 | 权重 | 加权分 |
|------|------|------|--------|
| 模块分离 | ⭐⭐⭐⭐⭐ | 20% | 1.0 |
| 测试覆盖 | ⭐⭐⭐⭐⭐ | 20% | 1.0 |
| 性能 | ⭐⭐ | 25% | 0.5 |
| API 设计 | ⭐⭐⭐ | 15% | 0.45 |
| 错误处理 | ⭐⭐ | 10% | 0.2 |
| 文档 | ⭐⭐⭐⭐ | 10% | 0.4 |
| **总分** | | | **3.55/5.0** |

---

## 二、关键问题清单（按严重性排序）

### 🔴 P0 - 关键问题（历史）

1. **Lock Delay 参数错误** - `types.rs`
   - 状态: 已修复
   - 当前: 500ms
   - 标准: 500ms (Tetris Guideline)
   - 影响: 高级技巧（stalled techniques）执行困难
   - 修复: 1 行代码

2. **B2B 奖励计算错误** - `scoring.rs`
   - 状态: 已修复
   - 当前: `(base_score + combo_score) / 2`
   - 标准: `(base_score + combo_score) / 2`
   - 影响: 连击+B2B 得分偏低，影响高分策略
   - 修复: 3 行代码

3. **Board 消行内存分配** - `board.rs`
   - 状态: 已修复
   - 当前: `ArrayVec<usize, 4>`（栈分配）
   - 影响: 热路径分配，GC 压力
   - 修复: 使用 `ArrayVec<usize, 4>`（栈分配）
   - 工作量: 5 行代码 + 添加依赖

4. **Vec<Vec<Cell>> 双重间接** - `board.rs`
   - 状态: 已修复
   - 当前: 扁平数组
   - 影响: 每次访问 cell 需两次内存跳转
   - 修复: 扁平化为 `[Cell; 200]` 一维数组
   - 工作量: 50 行代码重构

5. **UI 全量重绘**
   - 状态: 已修复
   - 当前: framebuffer diff flush
   - 影响: 60fps 时 CPU 占用高
   - 修复: 实现增量渲染（只绘制变化的 cell）
   - 工作量: 100 行代码

### 🟡 P1 - 重要问题（影响手感和功能完整度）

6. **DAS/ARR 系统缺失** - 全新模块
   - 状态: 已修复（输入层已实现）
   - 标准: DAS 167ms, ARR 33ms
   - 影响: 无法快速移动，Finesse 技巧不可用
   - 修复: 新增 `InputHandler` 模块
   - 工作量: 150 行代码

7. **ARE (Entry Delay) 缺失** - `game_state.rs`
   - 当前: 无实现
   - 标准: 消行后入场延迟（即使为 0ms 也应显式）
   - 影响: 消行后手感生硬
   - 修复: 添加 `are_timer_ms` 字段和状态机
   - 工作量: 30 行代码

8. **Soft Drop 重力错误** - `types.rs:11`
   - 状态: 已修复
   - 当前: 20x
   - 标准: 20-40x（通常固定约 50ms 间隔）
   - 影响: 软降速度偏慢
   - 修复: 1 行常量修改

9. **GameState 封装不足** - `game_state.rs:56-80`
   - 问题: 所有字段 `pub`，外部可直接修改
   - 影响: 可能破坏状态不变式
   - 修复: 私有字段 + getter 方法
   - 工作量: 100 行代码

10. **Adapter TCP Server 未实现** - 全新模块
    - 状态: 已修复（tokio server + e2e/acceptance/closed-loop tests）

### 🟢 P2 - 完善性问题

- ARE 参数微调（即使设为 0ms）
- 音效支持（rodio）
- 高分榜系统
- 主题/配色配置
- 详细统计（APM/PPS）

---

## 三、三层级改进建议

### ⚡ 立即执行（今天，1-2 小时）

（已完成；保留原清单作为历史记录）

目标: 修复游戏平衡性，达到 95%+ Guideline 兼容

| 任务 | 文件 | 改动量 | 风险 |
|------|------|--------|------|
| 1. Lock Delay 500ms | `types.rs:13` | 1 行 | 无 |
| 2. B2B 奖励含 Combo | `scoring.rs:71` | 3 行 | 低 |
| 3. Soft Drop 20x | `types.rs:11` | 1 行 | 无 |
| 4. 添加 ArrayVec 依赖 | `Cargo.toml` | 2 行 | 无 |
| 5. 修复 Board 消行分配 | `game_state.rs` | 5 行 | 低 |

**验收标准**:
- [ ] `cargo test` 全部通过
- [ ] 游戏手感测试正常
- [ ] 计时参数符合 Guideline

**预计时间**: 2-4 小时（含测试）

### 📅 本周完成（3-5 天）

目标: 完成核心性能优化和 DAS/ARR

| 任务 | 文件 | 工作量 | 依赖 |
|------|------|--------|------|
| 1. Board 扁平化数组 | `board.rs` | 50 行 | 无 |
| 2. ArrayVec 消行 | `game_state.rs` | 5 行 | arrayvec |
| 3. GameState 封装 | `game_state.rs` | 100 行 | 无 |
| 4. InputHandler | 新文件 | 150 行 | 无 |
| 5. DAS/ARR 集成 | `main.rs` | 30 行 | InputHandler |
| 6. ARE 框架 | `game_state.rs` | 30 行 | 无 |
| 7. Criterion 基准 | `benches/` | 50 行 | criterion |

**验收标准**:
- [ ] `cargo bench` 通过（tick <100μs, clear <10μs）
- [ ] DAS/ARR 手感测试正常
- [ ] heaptrack 显示零分配/帧
- [ ] 测试覆盖保持 >90%

**预计时间**: 3-5 天

### 🗓️ 长期规划（2-3 周）

目标: 生产级品质，100% AI 兼容

| 阶段 | 内容 | 天数 | 累计 |
|------|------|------|------|
| Phase 4 | Adapter TCP Server | 3-5 | 8-10 |
| Phase 5 | 音效 + 高分榜 | 2-3 | 10-13 |
| Phase 6 | 增量渲染优化 | 2-3 | 12-16 |
| 验收 | 全量测试 + 文档 | 1-2 | 13-18 |

**关键里程碑**:
1. **Week 2 结束**: AI 客户端可连接并控制游戏
2. **Week 3 结束**: 与 swiftui-tetris 100% 协议兼容
3. **Week 3 结束**: 零崩溃、零内存泄漏

---

## 四、总工作量估算

### 4.1 按阶段分解

| 阶段 | 任务数 | 代码行数 | 测试数 | 天数 | 风险等级 |
|------|--------|----------|--------|------|----------|
| 1. 游戏平衡 | 5 | ~50 | +10 | 1-2 | 低 |
| 2. 性能优化 | 5 | ~300 | +20 | 2-3 | 中 |
| 3. DAS/ARR | 3 | ~150 | +10 | 1-2 | 中 |
| 4. TCP Server | 4 | ~500 | +15 | 3-5 | 高 |
| 5. 打磨 | 4 | ~300 | +5 | 2-3 | 低 |
| **总计** | **21** | **~1,300** | **+60** | **9-15** | - |

### 4.2 人月估算

**保守估计**: 15 工作日（3 周）  
**乐观估计**: 9 工作日（2 周）  
**缓冲时间**: 3 工作日（意外问题）

**推荐计划**: **12 工作日（2.5 周）**

### 4.3 资源需求

| 资源 | 数量 | 说明 |
|------|------|------|
| 开发者 | 1 人 | 熟悉 Rust + 异步 |
| AI 客户端 | 1 个 | 用于 Adapter 测试 |
| 测试环境 | 1 套 | Linux/macOS |

---

## 五、风险与缓解

| 风险 | 概率 | 影响 | 缓解措施 |
|------|------|------|----------|
| Board 重构引入 Bug | 中 | 高 | 完整回归测试（16 个 board 测试） |
| DAS/ARR 手感不佳 | 中 | 中 | 参数可调（配置文件） |
| TCP Server 复杂度 | 高 | 中 | 先实现简化版（单 controller） |
| 性能优化收益不足 | 低 | 低 | 基准测试验证 |
| ARE 与现有逻辑冲突 | 低 | 中 | 渐进式集成，先设为 0ms |

---

## 六、验收标准

### 6.1 功能验收

- [ ] 所有 Guideline 计时参数正确（Lock 500ms, DAS 167ms, ARR 33ms）
- [ ] B2B + Combo 计分测试通过
- [ ] DAS/ARR 长按方向键持续移动
- [ ] AI 客户端 100% 兼容 swiftui-tetris
- [ ] 零崩溃，所有边界情况处理

### 6.2 性能验收

- [ ] `cargo bench` 通过:
  - tick: < 100μs
  - clear: < 10μs
  - render: < 1ms
- [ ] heaptrack 显示 <1 分配/帧
- [ ] 1 小时连续运行无内存泄漏
- [ ] 60 FPS 稳定（<16ms/帧）

### 6.3 代码验收

- [ ] 新增代码 80%+ 测试覆盖
- [ ] Clippy 零警告
- [ ] 文档完整（所有 pub API）
- [ ] 代码审查通过

---

## 七、立即开始行动清单

### 今天（2 小时）

```bash
# 1. 创建分支
git checkout -b fix/game-balance

# 2. 修复 Lock Delay
# 文件: src/types.rs:13
# 修改: 450 -> 500

# 3. 修复 B2B
cargo test scoring  # 确认测试失败
cargo test scoring  # 修改后确认通过

# 4. 修复 Soft Drop
cargo test

# 5. 提交
git add .
git commit -m "Fix: Lock delay 500ms, B2B bonus calculation, Soft drop 20x"
```

### 本周目标

- [ ] 完成 Phase 1（游戏平衡）
- [ ] 开始 Phase 2（Board 重构）
- [ ] 添加 ArrayVec 依赖
- [ ] 运行基准测试基线

---

## 八、总结

**当前状态**: 优秀的 MVP，架构清晰，测试覆盖良好

**主要差距**:
1. 计时参数微调（1 天）
2. 性能优化（3 天）
3. DAS/ARR（2 天）
4. TCP Server（4 天）

**预期成果**:
- 游戏性: 98% Guideline 兼容
- 性能: 零分配，<1ms 渲染
- 功能: 100% AI 兼容

**推荐启动顺序**:
1. **今天**: Phase 1（游戏平衡）
2. **本周**: Phase 2（性能）+ Phase 3（DAS/ARR）
3. **下周**: Phase 4（Adapter）
4. **随后**: Phase 5（打磨）

**预计总工作量**: **9-15 工作日（2-3 周）**

---

*评估完成*  
*文档依据: architecture-review.md, IMPROVEMENT-PLAN.md, TODO.md*
