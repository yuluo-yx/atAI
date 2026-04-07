# `@ai`

`@ai` 是一个本地命令行工具，用来把自然语言需求转换成候选 shell 命令，先展示给用户审查，只有在用户明确确认后才会执行。

<p style="align:center">
  <img src="./demo.gif"/>
</p>

这个项目由 AI 协助构建。AI 负责帮助生成代码和交互流程，但这并不意味着生成的命令天然安全；是否执行，最终仍由用户决定。

项目当前拆分为 3 个 Rust crate：

- `crates/atai-core`：核心逻辑，包括配置加载、模型调用、风险策略、执行器与历史记录
- `crates/atai-cli`：命令入口、子命令解析与主流程编排
- `crates/atai-tui`：终端界面层，负责命令审阅与确认交互

## 目标

- 把自然语言需求转换成可审阅的 shell 命令
- 在执行前应用本地风险检查
- 支持在执行前快速重新生成同一请求对应的命令
- 把最终执行权留给用户，而不是自动运行命令

> 提示：配置模型时，优先选择响应速度更快的模型并关闭思考模式，例如 `qwen3.5-flash`。

## 运行时文件

运行时目录固定为：

```text
~/.@ai/
```

目录中至少需要包含以下文件：

```text
config.toml
system_prompt.txt
command_denylist.txt
command_confirmlist.txt
```

`atai` 启动时会检查这些文件。如果缺少任何一个文件，程序会停止并提示你先运行 `atai config init`。

## 安全模型

工具使用两层控制：

- 外部 system prompt：从 `~/.@ai/system_prompt.txt` 读取
- 本地策略校验：在执行前强制生效，不依赖模型自我申报

另外，运行时还会附加少量内置约束，用于减少明显不适合当前平台或不利于人工审查的命令：

- 按当前平台默认工具行为生成命令，避免随意套用 GNU 独有参数形式
- 查看体积、磁盘占用等场景时，默认优先使用更适合人读的输出格式，例如 `-h`
- 优先生成更短、更容易人工核对的命令

### 直接拒绝规则

如果命令命中以下任一条件，会被直接拒绝：

- 动态执行：`eval`、`source`、反引号、`$()`
- 不支持的 shell 特性：here-doc、后台执行、shell 函数定义、多行命令、`;` 串联命令
- 灾难性删除模式，例如 `rm -rf /`、`rm -rf ~`、`rm -rf $HOME`
- 命中 `command_denylist.txt` 中的任意关键字

### 额外确认规则

如果命令命中以下任一条件，会被展示但需要额外确认：

- 输出重定向到普通文件，例如 `>` 或 `>>`
- 可能写入当前工作目录之外的路径
- 命中 `command_confirmlist.txt` 中的任意关键字

## 确认逻辑

- 安全命令：按 `Enter` 立即执行
- 高风险命令：先按一次 `Enter` 进入确认，再按一次 `Enter` 执行
- 被拒绝命令：不会执行；使用 `Ctrl+R` 重新生成，或按 `Enter` 关闭预览
- 按 `Ctrl+E`：作为备用执行键
- 按 `Ctrl+C` 或 `Ctrl+Q`：立即退出，不执行命令

## 配置

默认配置文件路径：

```toml
~/.@ai/config.toml
```

配置示例：

```toml
[model]
endpoint = "https://api.openai.com/v1"
model = "gpt-5.4"
api_key = "${OPENAI_API_KEY}"
timeout_seconds = 60
# enable_thinking = false

[execution]
shell = "/bin/zsh"

[safety]
mode = "tiered"

[history]
enabled = false
max_entries = 200
redact_paths = true
```

### `api_key` 解析规则

- `api_key = "${OPENAI_API_KEY}"`：从环境变量 `OPENAI_API_KEY` 读取
- `api_key = "sk-xxx"`：直接把字符串字面量当作密钥使用

推荐优先使用环境变量，这样真实密钥不会直接落盘。

### `enable_thinking` 说明

- 这是一个可选配置；默认省略时，保持模型或网关自己的默认行为。
- 当配置为 `false` 时：
  - 对 OpenAI `Responses API` 风格请求，会映射为 `reasoning.effort = "none"`，用于降低时延和思考 Token 消耗。
  - 对 DashScope 兼容网关，会透传根级 `enable_thinking = false`。
- 当配置为 `true` 时：
  - 对支持该扩展字段的兼容网关，会透传 `enable_thinking = true`。
  - 对标准 OpenAI `Responses API`，不会强行覆盖模型默认思考级别。

> 注意：当前工具仍然调用 `/responses`，不是 `/chat/completions`。如果某个兼容网关只支持后者，仅添加 `enable_thinking` 配置并不能让该网关直接可用。

## 命令

安装完成后的主要使用方式：

```bash
@ai 帮我找出当前目录下最大的 5 个文件夹
```

内置命令：

```bash
atai version
atai config
atai config show
atai config init
```

- `atai version`：输出当前二进制版本
- `atai config` / `atai config show`：显示当前配置和运行时资源路径，`api_key` 会做脱敏展示
- `atai config init`：初始化 `~/.@ai` 目录，并生成 `config.toml`、`system_prompt.txt`、命令黑名单和确认列表

初始化示例：

```bash
mkdir -p ~/.@ai
atai config init
atai config show
```

如果你只想生成命令而不进入 TUI，也可以这样运行：

```bash
atai --print-only 帮我找出当前目录下最大的 10 个文件
```

### 行内预览快捷键

- `Enter`：执行命令；高风险命令需要按两次
- `Ctrl+E`：备用执行键；高风险命令也需要按两次
- `Ctrl+R`：针对同一个请求重新生成候选命令
- `Ctrl+C` / `Ctrl+Q`：立即退出，不执行命令
- `Esc`：取消高风险执行确认

## 安装

使用 `curl` 安装最新 release：

```bash
curl -fsSL https://raw.githubusercontent.com/yuluo-yx/atAI/main/install.sh | sh
```

安装脚本会：

- 从 GitHub Releases 下载与当前平台匹配的最新二进制
- 从同一个 tag 的源码树中下载 `@ai` 包装脚本
- 默认把 `atai` 和 `@ai` 安装到 `~/.local/bin`
- 仅在运行时必需文件缺失时自动执行一次 `atai config init`
- 如果 `~/.@ai` 已经初始化完成，则保留现有配置并跳过初始化

可选环境变量：

```bash
ATAI_INSTALL_VERSION=2026.04.07 \
ATAI_INSTALL_DIR="$HOME/.local/bin" \
ATAI_INSTALL_REPO="yuluo-yx/atAI" \
sh install.sh
```

如果 `~/.local/bin` 不在你的 `PATH` 中，需要先把它加进去。

你也可以继续使用源码方式本地安装：

```bash
make help
make build
make build BUILD_TARGET=aarch64-apple-darwin
make build-all
make fmt-check
make check
make test
make fmt
make clippy
make verify
make install
```

默认安装目录是 `~/.local/bin`：

- `atai`：Rust 编译出的主程序
- `@ai`：一个包装脚本，会转发到同目录下的 `atai`

`make build-all` 会按 release 目标矩阵逐个构建，遇到缺少 Rust target、链接器或系统工具链时会立即失败。`x86_64-pc-windows-msvc` 通常需要在带有 MSVC 工具链的 Windows 主机上构建。

`make install` 继续保持宿主平台安装语义，不负责安装跨平台构建产物。

GitHub Releases 现在直接发布原始二进制，不再生成 `.tar.gz` 或 `.zip` 归档。

## 许可证

本项目采用 GNU GPL v3.0 only 许可证。详见 [LICENSE](/Users/shown/workspace/@AI/LICENSE)。
