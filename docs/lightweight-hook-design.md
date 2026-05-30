# 轻量级 QQ Hook 架构设计

**目标**: 不依赖 NapCat，自己实现 QQ NT 协议桥接

## 核心发现

### NapCat 的 Hook 机制

1. **不是传统 DLL 注入**，而是利用 QQ NT 本身就是 Node.js 应用
2. `NapCatWinBootHook.dll` 修改 QQ 的 `package.json`，将 main 改为 `loadNapCat.js`
3. QQ 自己的 Node.js 运行时加载 NapCat 的 JS 代码

### 关键组件

| 组件 | 路径 | 功能 |
|------|------|------|
| `wrapper.node` | QQ 安装目录 | QQ 原生模块包装器 |
| `MoeHoo.node` | NapCat/native/ | 网络包 Hook |
| 命名管道 | `\\.\pipe\NapCat_{pid}` | IPC 通信 |

### wrapper.node 暴露的服务

```javascript
// 登录服务
NodeIKernelLoginService
  - quickLoginWithUin(uin)        // 快速登录
  - getLoginInfo()                 // 获取登录信息

// 消息服务  
NodeIKernelMsgService
  - sendMessage(peer, content)    // 发送消息
  - getRecentChatList()            // 获取会话列表

// NTEventWrapper
// 事件分发，监听登录状态、消息接收等
```

## 架构设计

```
┌─────────────────────────────────────────────────────────┐
│                      qqcli-rs (Rust CLI)                 │
│  ┌─────────┐  ┌─────────┐  ┌─────────┐  ┌─────────┐   │
│  │  Send   │  │ Contact │  │ Message │  │  OneBot │   │
│  │ Message │  │  List   │  │ History │  │  Bridge │   │
│  └────┬────┘  └────┬────┘  └────┬────┘  └────┬────┘   │
│       └────────────┴─────────────┴─────────────┘        │
│                         │                               │
│                    IPC (TCP/WS)                         │
└─────────────────────────┼───────────────────────────────┘
                          │
┌─────────────────────────┼───────────────────────────────┐
│                  QQHook.dll (C++/Rust)                  │
│  ┌─────────┐  ┌─────────┐  ┌─────────┐                 │
│  │ DLL     │  │ Wrapper │  │  IPC    │                 │
│  │ Inject  │  │  Call   │  │ Server  │                 │
│  └─────────┘  └────┬────┘  └────┬────┘                 │
└─────────────────────┼────────────┼─────────────────────┘
                      │            │
┌─────────────────────┼────────────┼─────────────────────┐
│                     QQ.exe       │                       │
│  ┌──────────────────┴───────────┴────────────────┐    │
│  │            wrapper.node (QQ Native)              │    │
│  │  ┌──────────┐ ┌──────────┐ ┌──────────┐     │    │
│  │  │  Login   │ │  Message │ │  Event   │     │    │
│  │  │ Service  │ │ Service  │ │ Listener │     │    │
│  │  └──────────┘ └──────────┘ └──────────┘     │    │
│  └───────────────────────────────────────────────┘    │
└───────────────────────────────────────────────────────┘
```

## 实现方案

### 方案 A: 复用 wrapper.node (推荐)

**优点**:
- 不需要逆向网络协议
- 直接调用 QQ 的高层 API
- 相对稳定

**步骤**:
1. 写一个简单的 JS bridge (类似 loadNapCat.js)
2. 复用 NapCat 的 DLL injector 或自己写
3. JS bridge 通过 node-ffi 调用 Rust IPC server
4. Rust CLI 通过 TCP/WS 与 bridge 通信

### 方案 B: 纯 Rust 实现

**优点**:
- 不需要 Node.js
- 完全可控

**缺点**:
- 需要自己解析 wrapper.node 的接口
- 复杂度高

## MVP 范围

### Phase 1: 发送消息
```
✓ 注入 DLL 到 QQ.exe
✓ 加载 wrapper.node
✓ 调用 quickLoginWithUin()
✓ 调用 sendMessage()
✓ 通过 TCP/WebSocket 暴露 API
```

### Phase 2: 联系人列表
```
✓ getRecentChatList()
✓ getFriendList()
✓ getGroupList()
```

### Phase 3: 事件接收
```
✓ 监听消息事件
✓ 实时推送到客户端
```

## 技术选型

### DLL Injector
```rust
// 使用 windows-rs
use windows::Win32::Foundation::*;
use windows::Win32::System::Threading::*;

pub fn inject_dll(pid: u32, dll_path: &str) -> Result<()> {
    // 1. OpenProcess
    // 2. VirtualAllocEx (分配内存)
    // 3. WriteProcessMemory (写入 DLL 路径)
    // 4. CreateRemoteThread + LoadLibrary
}
```

### IPC 通信
- **TCP**: 简单直接，Rust 原生支持
- **WebSocket**: 兼容 OneBot11 客户端

### JS Bridge
```javascript
// hook.js
const { connect } = require('ffi-rs');
const net = require('net');

// 连接到 Rust IPC server
const client = net.connect(9333, '127.0.0.1');

process.on('message', (cmd) => {
    if (cmd.type === 'send_message') {
        const msgService = wrapper.getKernelMsgService();
        msgService.sendMessage(cmd.peer, cmd.content);
    }
});
```

## 依赖 NapCat 什么

1. **wrapper.node** - 直接从 QQ 安装目录复制
2. **DLL injector 逻辑** - 可以参考 NapCat 的实现
3. **JS bridge 模板** - 简化版

## 时间估算

| Phase | 任务 | 复杂度 | 时间 |
|--------|------|--------|------|
| 1 | DLL Injector | ⭐⭐ | 1-2 天 |
| 1 | JS Bridge | ⭐⭐ | 1 天 |
| 1 | IPC Server | ⭐⭐ | 1 天 |
| 1 | Rust CLI 集成 | ⭐⭐ | 1 天 |
| 2 | Contact List | ⭐⭐⭐ | 1-2 天 |
| 3 | Event Listener | ⭐⭐⭐⭐ | 2-3 天 |

## 下一步

1. 先写一个 PoC: 启动 QQ + 注入 + 打印 "Hello from hook"
2. 验证 injector 工作正常
3. 然后逐步增加功能

## 参考

- NapCat source: `/c/Users/Administrator/NapCat/napcat/`
- wrapper.node 调用示例: napcat.mjs 第 116000+ 行
