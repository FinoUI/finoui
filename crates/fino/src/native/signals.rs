//! Per-window invalidation and frame request state shared by DOM, layout, and host.
//! 由 DOM、布局与 Host 共享的每窗口失效与帧请求状态。

use std::cell::Cell;

use serde::Serialize;

use super::id::WindowId;

/// Coalesced frame and invalidation state owned by exactly one [`WindowId`].
/// 由唯一 [`WindowId`] 持有的合并帧与失效状态。
#[derive(Debug)]
pub struct WindowSignals {
    window: WindowId,
    frame_requested: Cell<bool>,
    frame_requests: Cell<u64>,
    layout_dirty: Cell<bool>,
    paint_dirty: Cell<bool>,
    accessibility_dirty: Cell<bool>,
    layout_generation: Cell<u64>,
    paint_generation: Cell<u64>,
    committed_frames: Cell<u64>,
}

impl WindowSignals {
    /// Creates signals for one window.
    /// 为一个窗口创建信号。
    pub fn new(window: WindowId) -> Self {
        Self {
            window,
            frame_requested: Cell::new(false),
            frame_requests: Cell::new(0),
            layout_dirty: Cell::new(true),
            paint_dirty: Cell::new(true),
            accessibility_dirty: Cell::new(true),
            layout_generation: Cell::new(0),
            paint_generation: Cell::new(0),
            committed_frames: Cell::new(0),
        }
    }

    /// Returns the owning window.
    /// 返回所属窗口。
    pub fn window(&self) -> WindowId {
        self.window
    }

    /// Requests one frame, coalescing repeated requests until it is taken.
    /// 请求一帧，并在被取走前合并重复请求。
    pub fn request_frame(&self) {
        if !self.frame_requested.replace(true) {
            self.frame_requests.set(self.frame_requests.get() + 1);
        }
    }

    /// Takes the pending request without scheduling another frame.
    /// 取走待处理请求，不额外调度帧。
    pub fn take_frame_request(&self) -> bool {
        self.frame_requested.replace(false)
    }

    /// Returns whether a frame request is currently pending.
    /// 返回当前是否有待处理帧请求。
    pub fn frame_request_pending(&self) -> bool {
        self.frame_requested.get()
    }

    /// Returns the number of coalesced frame requests issued so far.
    /// 返回迄今发出的合并帧请求数。
    pub fn frame_requests(&self) -> u64 {
        self.frame_requests.get()
    }

    /// Marks structural, text, or constraint changes that require layout and paint.
    /// 标记需要重新布局与绘制的结构、文本或约束变化。
    pub fn mark_layout_dirty(&self) {
        self.layout_dirty.set(true);
        self.paint_dirty.set(true);
        self.request_frame();
    }

    /// Marks paint-only changes that do not require layout.
    /// 标记无需布局的纯绘制变化。
    pub fn mark_paint_dirty(&self) {
        self.paint_dirty.set(true);
        self.request_frame();
    }

    /// Marks accessibility-only changes without requesting a frame.
    /// 标记仅无障碍变化，不请求帧。
    pub fn mark_accessibility_dirty(&self) {
        self.accessibility_dirty.set(true);
    }

    /// Takes the layout dirty flag.
    /// 取走布局失效标记。
    pub fn take_layout_dirty(&self) -> bool {
        self.layout_dirty.replace(false)
    }

    /// Takes the paint dirty flag.
    /// 取走绘制失效标记。
    pub fn take_paint_dirty(&self) -> bool {
        self.paint_dirty.replace(false)
    }

    /// Takes the accessibility dirty flag.
    /// 取走无障碍失效标记。
    pub fn take_accessibility_dirty(&self) -> bool {
        self.accessibility_dirty.replace(false)
    }

    /// Increments and returns the committed layout generation.
    /// 递增并返回已提交布局代次。
    pub fn commit_layout(&self) -> u64 {
        let generation = self.layout_generation.get() + 1;
        self.layout_generation.set(generation);
        generation
    }

    /// Increments and returns the committed paint generation.
    /// 递增并返回已提交绘制代次。
    pub fn commit_paint(&self) -> u64 {
        let generation = self.paint_generation.get() + 1;
        self.paint_generation.set(generation);
        generation
    }

    /// Records one presented frame.
    /// 记录一个已呈现帧。
    pub fn commit_frame(&self) {
        self.committed_frames.set(self.committed_frames.get() + 1);
        self.frame_requested.set(false);
        self.layout_dirty.set(false);
        self.paint_dirty.set(false);
    }

    /// Returns the number of presented frames.
    /// 返回已呈现帧数。
    pub fn committed_frames(&self) -> u64 {
        self.committed_frames.get()
    }

    /// Returns a stable diagnostic snapshot.
    /// 返回稳定诊断快照。
    pub fn snapshot(&self) -> WindowSignalsSnapshot {
        WindowSignalsSnapshot {
            window: self.window,
            frame_requested: self.frame_requested.get(),
            frame_requests: self.frame_requests.get(),
            layout_dirty: self.layout_dirty.get(),
            paint_dirty: self.paint_dirty.get(),
            accessibility_dirty: self.accessibility_dirty.get(),
            layout_generation: self.layout_generation.get(),
            paint_generation: self.paint_generation.get(),
            committed_frames: self.committed_frames.get(),
        }
    }
}

/// Serializable per-window invalidation state.
/// 可序列化的每窗口失效状态。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct WindowSignalsSnapshot {
    /// Owning window.
    /// 所属窗口。
    pub window: WindowId,
    /// Whether a frame is pending.
    /// 是否有待处理帧。
    pub frame_requested: bool,
    /// Coalesced request count.
    /// 合并请求数。
    pub frame_requests: u64,
    /// Layout dirty flag.
    /// 布局失效标记。
    pub layout_dirty: bool,
    /// Paint dirty flag.
    /// 绘制失效标记。
    pub paint_dirty: bool,
    /// Accessibility dirty flag.
    /// 无障碍失效标记。
    pub accessibility_dirty: bool,
    /// Committed layout generation.
    /// 已提交布局代次。
    pub layout_generation: u64,
    /// Committed paint generation.
    /// 已提交绘制代次。
    pub paint_generation: u64,
    /// Presented frame count.
    /// 已呈现帧数。
    pub committed_frames: u64,
}
