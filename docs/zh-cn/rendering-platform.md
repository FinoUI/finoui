[English](../en/rendering-platform.md) · [简体中文](rendering-platform.md)

# 渲染与平台 Host

## 原生渲染与 GPU 扩展

### Display List

```rust
pub enum DrawCommand {
    Quad(DrawQuad),
    Text(DrawText),
    Path(DrawPath),
    Material(DrawMaterial),
    Effect(ApplyEffect),
    GpuSurface(DrawGpuSurface),
    PushLayer(LayerSpec),
    PopLayer,
}
```

布局只产生几何与绘制描述，不直接调用 GPU。Renderer 负责排序、合批、缓存、离屏纹理与最终提交。

### 三类 Shader 能力

| 类型 | 用途 | 主要成本 |
|---|---|---|
| `Material` | 替代矩形或控件表面绘制 | Pipeline 切换、实例数据 |
| `Effect` | 对已绘制子树做模糊、扭曲等后处理 | Offscreen texture 与额外全屏 pass |
| `GpuSurface` | 粒子、图表、视频、3D 等自绘区域 | 独立渲染状态与同步 |

WGSL 是唯一自定义 Shader 语言。框架提供统一坐标、颜色空间、预乘 alpha、抗锯齿与 uniform 约定，不让开发者从完整 pipeline 开始写。

### Signal 驱动 uniform

```text
Signal 变化
→ 更新对应 MaterialInstance 的 uniform
→ 标记 Paint
→ 请求帧
```

此路径不得重新执行组件、重新创建节点、重新 shape 文本或重新布局。动画只有活跃时才请求连续帧；静止后恢复按需渲染。

### Web 能力映射

| 原生效果 | Web 后端策略 |
|---|---|
| 渐变、圆角、阴影、简单 filter | 转译为 CSS |
| 部分可表达效果 | SVG |
| 任意 WGSL Material / Effect | 节点内局部 WebGPU Canvas，或声明式降级 |
| GpuSurface | WebGPU / WebGL Canvas |

普通 UI 始终是实体 DOM。局部 GPU Canvas 是显式能力，不得悄悄把整个应用退化为 Canvas。

## 平台 Host 与服务

### Host 职责

Host 只负责：

- 创建与驱动应用生命周期。
- 提供窗口、Surface、事件循环与帧请求。
- 转换平台输入。
- 连接字体、IME、剪贴板、文件、无障碍等服务。

UI Core 不直接认识 Activity、UIKit、DOM Window 或 winit Window。

### 统一生命周期

```text
HostCreated
SurfaceReady
SurfaceLost
Suspended
Resumed
HostDestroyed
```

UI 状态、DOM 与 ShapedText 必须能在 Surface 丢失后保留。GPU Device、Surface 与交换链资源由 Host/Renderer 重建。

### 输入与语义

公共输入模型支持鼠标、触摸、触控笔、键盘、IME 与焦点，同时保留平台差异。移动端软键盘、系统返回、安全区域和文本选区由 Host 与平台服务协作，不通过自绘键盘绕过系统能力。

原生语义树从 DOM-like 结构生成并映射到 AccessKit；Web 默认使用真实 DOM 语义。SharedLayout Web 模式仍需单独验证 DOM 顺序、焦点和文本选择。
