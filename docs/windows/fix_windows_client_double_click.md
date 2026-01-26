# Fix: Windows Client Double-Click & Drag Failure

## 问题描述
用户在 Windows 客户端运行 `platform-passer` 时，发现桌面程序无法双击打开，且无法在桌面上拖拽框选。此外，右键弹出菜单后，左键点击空白处无法关闭菜单。

### 根因分析
1.  **输入钩子阻塞 (Hook Blocking)**:
    - 之前的实现中，输入回调函数使用 `blocking_send` 向网络通道发送事件。当网络拥塞或缓冲区满时，这会直接阻塞 Windows 的底层输入钩子线程（WH_MOUSE_LL）。
    - Windows 对钩子线程有严格的时间限制。如果线程处理太慢，系统会通过丢弃后续事件（包括双击的第二次点击或鼠标抬起事件）来维持 UI 响应，导致应用层看到的点击“丢失”。
2.  **缺少“极速路径” (Hot Path)**:
    - 本地模式下，点击事件也经过了不必要的逻辑判断（如坐标转换、锁竞争），增加了延迟。
3.  **焦点状态死锁 (Focus State Deadlock)**:
    - 客户端在接收到来自服务端的焦点切换信号时，没有完全清除“远程控制（吞掉输入）”的状态，导致它在本地操作时依然在拦截点击。
4.  **Handshake ID 错误标识**:
    - 之前 Handshake 硬编码为 `macos-client`，导致服务端日志混淆，不便于针对性排查 Windows 特有输入问题。

## 修复方案

### 1. 通道发送异步化 (client.rs)
将 `blocking_send` 替换为 `try_send`。 捕获线程绝不能因为网络状态而停下。
```rust
// crates/session/src/client.rs
if input_tx.try_send(Frame::Input(event)).is_err() {
    // 即使发送失败（溢出），也不允许阻塞 Hook 线程
}
```

### 2. 引入鼠标本地“极速通道” (source.rs)
在 `mouse_proc` 的最开始，如果是本地模式且不是移动事件，直接放行，不进行任何加锁或逻辑计算。
```rust
// crates/input/src/windows/source.rs
if !is_remote && msg != WM_MOUSEMOVE {
    return CallNextHookEx(MOUSE_HOOK, code, wparam, lparam);
}
```

### 3. 全面使用尝试锁 (try_lock)
在钩子回调中，禁止使用 `lock().unwrap()`。如果锁被占用，立即跳过该帧（对于 Move 事件）或放行原语（对于 Click 事件），确保实时性。

### 4. 强制状态重置 (session/server.rs & client.rs)
调整 `ScreenSwitch` 事件的处理逻辑。当任何一端接收到焦点切换指令（无论来源），立即强制 `source.set_remote(false)`，确保当前拥有物理输入的设备不会拦截本地事件。

### 5. 动态 Handshake ID
根据操作系统动态生成 Handshake ID（如 `windows-client`），方便后端日志分析。

## 验证结论
- **双击性能**: 和系统原生表现一致。
- **拖拽选择**: 解决了由于 `LeftMouseUp` 丢失导致的拖拽无法结束问题。
- **系统菜单**: 右键菜单的交互逻辑恢复正常。
- **稳定性**: 解决了因主循环 `await` 内部消息导致的潜在死锁。

## 5. 最终根因：权限隔离 (UIPI) 与 隐形窗口干扰
经过深度排查，导致 Windows Desktop 点击失效的最后两个关键拼图是：
1.  **隐形窗口遮挡**: 剪贴板监听器创了一个 `WS_OVERLAPPEDWINDOW` 样式的 0x0 窗口。在 Windows 桌面层（WorkerW）的点击测试中，这个“幽灵窗口”被误判为遮挡了桌面，导致左键点击（Left Click）无法穿透到桌面图标和菜单。
    - **修复**: 将其改为 `WS_POPUP` 且父窗口设为 `HWND_MESSAGE`，彻底移出屏幕 Z-order。
2.  **UIPI (User Interface Privilege Isolation)**: Windows 资源管理器（Explorer.exe）和桌面窗口通常运行在较高的完整性级别（Integrity Level）。如果 `platform-passer` 以普通用户权限运行，安装的底层鼠标钩子（WH_MOUSE_LL）会被系统静默限制，无法拦截或注入针对桌面的事件。
    - **解决方案**: 必须以 **管理员身份 (Run as Administrator)** 运行程序，或在最终构建时添加 `requireAdministrator` 清单文件。
