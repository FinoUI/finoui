# fino

[English](README.md) · [简体中文](README.zh-CN.md)

Fino 是一套以 Rust 实现的跨端 UI 框架，采用细粒度响应式模型，由真实 DOM 与原生 wgpu 渲染器驱动，默认使用 Solid Glass 主题。

## 文档

- [基础架构](docs/zh-cn/architecture.md)
- [多语言绑定](docs/zh-cn/bindings.md)
- [多窗口架构](docs/zh-cn/windows.md)
- [运行时与 DOM API](docs/zh-cn/runtime.md)
- [文本与布局](docs/zh-cn/text-layout.md)
- [渲染与平台 Host](docs/zh-cn/rendering-platform.md)
- [质量、性能与 CI](docs/zh-cn/quality.md)
- [依赖策略](docs/zh-cn/dependencies.md)
- [AI 原生开发](docs/zh-cn/ai-native-development.md)
- [演进路线与风险](docs/zh-cn/roadmap.md)

## 开发

安装 [pre-commit](https://pre-commit.com/) 并启用 Rust hooks：

```bash
pre-commit install
```

手动运行与 CI 相同的检查：

```bash
pre-commit run --all-files
```
