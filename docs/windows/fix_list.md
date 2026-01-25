# Windows Client - Bug Fix List

## Fix #1: Cursor Freeze After Multiple Switches or Restart

### 问题描述 (Problem Description)

**症状 (Symptoms):**
- 经过几次 Server ↔ Client 切换后，Windows Client 的本地鼠标和光标完全卡死
- 光标被锁定在屏幕中心，无法移动
- 本地鼠标设备完全无响应
- 只能通过强制关闭 Server 和 Client 程序才能恢复

**触发条件 (Trigger Conditions):**
- 多次在 Server 和 Client 之间切换
- Server 或 Client 异常重启/崩溃
- 鼠标意外移动到屏幕边缘

### 根本原因 (Root Cause)

**问题 1: Fallback 边缘检测逻辑**

在 `crates/input/src/windows/source.rs` 中，存在一个 Fallback 边缘检测逻辑：

```rust
// Line 244-246 (旧代码)
if GLOBAL_CONFIG.lock().unwrap().is_none() && abs_x >= 0.998 {
    trigger_remote = true;
}
```

**问题：**
- 即使没有配置 Topology，只要鼠标移动到右边缘（x >= 0.998），就会触发 Remote 模式
- Client 不应该有 Remote 模式！Client 应该始终处于 Local 模式

**问题 2: Center Locking 机制**

当 `IS_REMOTE = true` 时，Windows Source 使用"Center Locking"机制：

```rust
// Line 166
let mut swallow = is_remote;

// Line 207
let _ = SetCursorPos(center_x, center_y);  // 强制居中光标

// Line 276-278
if swallow {
    return LRESULT(1);  // 吞掉所有鼠标事件
}
```

**后果：**
1. 所有鼠标事件被吞掉（`swallow = true`），系统无法处理本地鼠标移动
2. 光标被强制锁定在屏幕中心
3. 如果 `IS_REMOTE` 状态没有正确重置（程序崩溃、异常断开），光标永久冻结

**问题 3: Client 启动时未强制设置 Local 模式**

在 `crates/session/src/client.rs` 中，Client 启动时没有调用 `source.set_remote(false)`，导致：
- `IS_REMOTE` 初始值虽然是 `false`（默认值）
- 但是边缘检测逻辑可能会将其设置为 `true`
- 一旦设置为 `true`，就会触发 Center Locking

### 修复方案 (Fix Solution)

**修复 1: 删除 Fallback 边缘检测**

文件：`crates/input/src/windows/source.rs`

```rust
// Line 243-248 (新代码)
}
// REMOVED: Fallback default edge detection
// This caused Client to accidentally enter Remote mode and freeze cursor
// Edge detection should ONLY happen when explicitly configured via Topology
// if GLOBAL_CONFIG.lock().unwrap().is_none() && abs_x >= 0.998 {
//     trigger_remote = true;
// }
```

**效果：**
- 现在只有在明确配置了 Topology 时才会触发边缘检测
- Client 不会因为鼠标移动到边缘而意外进入 Remote 模式

**修复 2: Client 启动时强制设置 Local 模式**

文件：`crates/session/src/client.rs`

```rust
// Line 82-92 (新代码)
// Start Input Capture Once (Server receives events from Client)
let input_tx = local_tx.clone();
let input_log = event_tx.clone();
if let Err(e) = source.start_capture(Box::new(move |event| {
    let _ = input_tx.blocking_send(Frame::Input(event));
})) {
    log_error!(&input_log, "Failed to start input capture: {}", e);
}

// CRITICAL: Client should NEVER enter Remote mode (edge detection disabled)
// Force Local mode to prevent cursor freeze
let _ = source.set_remote(false);
```

**效果：**
- Client 启动时强制设置为 Local 模式
- 确保 `IS_REMOTE = false`，防止意外进入 Remote 模式

### 验证步骤 (Verification Steps)

1. **重新编译 Windows Client**
2. **测试多次切换**：
   - 在 macOS Server 和 Windows Client 之间切换 10+ 次
   - 确认 Windows 本地鼠标始终可用
3. **测试异常重启**：
   - 强制关闭 macOS Server（模拟崩溃）
   - 确认 Windows Client 的鼠标不会冻结
   - 重新启动 Server 并连接
4. **测试边缘移动**：
   - 在 Windows 上将鼠标移动到屏幕边缘
   - 确认不会触发 Remote 模式
   - 确认本地鼠标仍然可用

### 相关文件 (Related Files)

- `crates/input/src/windows/source.rs` - Windows 输入捕获（边缘检测逻辑）
- `crates/session/src/client.rs` - Client 会话管理（启动逻辑）
- `crates/input/src/traits.rs` - InputSource trait 定义

### 修复日期 (Fix Date)

2026-01-25

### 修复人员 (Fixed By)

Antigravity AI Assistant
