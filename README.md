# ipport

用 Rust 查询本机 TCP/UDP 端口与所属进程，同时提供 CLI 和本地 Web 页面。面向 **macOS 和 Windows**，通过原生系统接口采集数据，无需安装或调用 `lsof`、`netstat` 等命令。

## 快速开始

需要 Rust 1.95 或更新版本。macOS 构建需要 Xcode Command Line Tools（`xcode-select --install`，提供 Clang 和系统头文件）；Windows 使用 MSVC 工具链，需要 Visual Studio Build Tools 的 C++ 构建工具和 Windows SDK。运行已编译的二进制无需这些构建工具。

```sh
# 默认：TCP 监听端口 + 已绑定的 UDP 端口
cargo run --

# 过滤查询
cargo run -- list --port 8080
cargo run -- list --protocol tcp --search node
cargo run -- list --pid 1234

# 端口详情：包含该本地端口的活动 TCP 连接
cargo run -- show 8080

# 机器可读输出 / 全部 TCP 连接
cargo run -- list --json
cargo run -- list --all --json

# 本地 Web 页面：http://127.0.0.1:8787
cargo run -- web
cargo run -- web --bind 127.0.0.1:9000
```

Web 页面提供搜索、协议筛选、手动刷新、每 5 秒自动刷新和端口详情弹窗。页面及脚本内嵌在 Rust 二进制中，无需 Node.js 或单独启动前端服务。Web 服务只允许绑定回环地址。

## 查询内容与限制

- IPv4/IPv6、本地端口和地址、TCP 状态、TCP 远端地址。
- 所属进程 PID、名称、可执行文件路径和用户（可读取时）。同一 Socket 可关联多个进程。
- TCP 默认只展示 `LISTEN`；UDP 展示已绑定端口并标记 `BOUND`，不代表存在 TCP 式的监听状态。
- 同一端口可能对应多个地址、协议或 Socket，不会强行合并。
- 结果是即时快照，进程可能在扫描期间退出。普通权限可能无法读取其他用户或受保护的进程；macOS 某些进程的端口可能完全不可见。必要时以提升权限运行 CLI 获取更多信息。
- 绑定全部接口不代表可从公网访问，实际可访问性还取决于网络和防火墙。此工具查看本机 Socket，不进行远程端口扫描。
- Web 查询的是**运行 Rust 服务的电脑**，不是浏览器所在的远端设备。

## HTTP API

`GET /api/ports` 返回 `ports`、`platform`、`scanned_at_ms`、`elapsed_ms` 和 `warnings`。

```text
/api/ports?port=8080&protocol=tcp
/api/ports?pid=1234&all=true
/api/ports?search=node
```

筛选条件可组合；`protocol` 使用 `tcp` 或 `udp`，`all` 使用 `true` 或 `false`。无效参数返回 HTTP 400；扫描失败返回 500；已有扫描正在运行时返回 503，避免同时重复扫描。

## 开发与构建

```sh
cargo fmt --all -- --check
cargo clippy --all-targets --locked -- -D warnings
cargo test --locked
cargo build --release --locked
cargo install --path . --locked
```

安装后使用 `ipport`、`ipport show 8080` 或 `ipport web`。构建产物位于 `target/release/ipport`，Windows 为 `target/release/ipport.exe`。

`src/scanner.rs` 负责原生采集、进程关联和筛选；`src/main.rs` 负责 CLI；`src/web.rs` 提供 HTTP 接口；`assets/index.html` 为内嵌页面；`tests/` 包含创建真实 TCP/UDP Socket 的集成测试。CI 在 macOS 和 Windows 分别执行格式、Clippy、测试及发布构建。

采集依赖 [netstat2](https://docs.rs/netstat2/latest/netstat2/)；进程信息使用 [sysinfo](https://docs.rs/sysinfo/latest/sysinfo/)。
