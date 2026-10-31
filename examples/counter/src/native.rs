//! Native winit + wgpu host for the shared counter component.
//! 共享计数器组件的原生 winit + wgpu Host。

use std::{error::Error, sync::Arc};

use fino::{
    native::{
        Application, InputEvent, KeyInput, PointerAction, PointerInput, RenderStatus, Size,
        WindowContext, WindowDesc, WindowId,
    },
    reactive::Signal,
};
use serde_json::json;
use winit::{
    application::ApplicationHandler,
    dpi::{LogicalSize, PhysicalSize},
    event::{ElementState, Ime, MouseButton, WindowEvent as WinitWindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    keyboard::{Key, NamedKey},
    window::{Window, WindowId as WinitWindowId},
};

use crate::mount_counter_with_count;

/// Runs the real native counter window.
/// 运行真实原生计数器窗口。
pub fn run(smoke: bool) -> Result<(), Box<dyn Error>> {
    let event_loop = EventLoop::new()?;
    event_loop.set_control_flow(ControlFlow::Wait);
    let mut host = CounterHost::new(smoke);
    event_loop.run_app(&mut host)?;
    if let Some(failure) = host.failure {
        return Err(failure.into());
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SmokeState {
    Disabled,
    AwaitFirstFrame,
    AwaitSecondFrame,
    Done,
}

struct CounterHost {
    app: Option<Application>,
    window: Option<Arc<Window>>,
    winit_window: Option<WinitWindowId>,
    window_id: Option<WindowId>,
    root: Option<fino::native::NodeId>,
    count: Option<Signal<i32>>,
    pointer: fino::native::Point,
    smoke: SmokeState,
    smoke_initial_text: Option<String>,
    surface_presented: bool,
    failure: Option<String>,
}

impl CounterHost {
    fn new(smoke: bool) -> Self {
        Self {
            app: None,
            window: None,
            winit_window: None,
            window_id: None,
            root: None,
            count: None,
            pointer: fino::native::Point::default(),
            smoke: if smoke {
                SmokeState::AwaitFirstFrame
            } else {
                SmokeState::Disabled
            },
            smoke_initial_text: None,
            surface_presented: false,
            failure: None,
        }
    }

    fn app(&mut self) -> &mut Application {
        self.app
            .as_mut()
            .expect("application was created on resume")
    }

    fn ensure_window(&mut self, event_loop: &ActiveEventLoop) -> Result<(), String> {
        if self.window.is_some() {
            return Ok(());
        }
        let attributes = Window::default_attributes()
            .with_title("Fino Counter — Native (PR3)")
            .with_inner_size(LogicalSize::new(640.0, 360.0))
            .with_min_inner_size(LogicalSize::new(320.0, 200.0));
        let window = Arc::new(
            event_loop
                .create_window(attributes)
                .map_err(|error| error.to_string())?,
        );
        window.set_visible(true);
        window.focus_window();
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_with_display_handle(
            Box::new(event_loop.owned_display_handle()),
        ));
        let surface = instance
            .create_surface(Arc::clone(&window))
            .map_err(|error| error.to_string())?;
        let mut app =
            Application::with_surface(instance, &surface).map_err(|error| error.to_string())?;
        let window_desc = WindowDesc {
            title: "Fino Counter — Native (PR3)".to_owned(),
            width: 640,
            height: 360,
        };
        let window_id = app.create_window(window_desc);
        if let Some(context) = app.window_mut(window_id) {
            apply_initial_viewport(context, window.inner_size(), window.scale_factor());
        }
        app.install_surface(window_id, surface)
            .map_err(|error| error.to_string())?;
        let count = app.owner().signal_named("counter/count", 0_i32);
        let mount_count = count.clone();
        let root = app
            .mount_window(window_id, move |runtime, owner| {
                mount_counter_with_count(runtime, owner, mount_count)
            })
            .ok_or_else(|| "window vanished while mounting the counter".to_owned())?;
        self.winit_window = Some(window.id());
        self.window_id = Some(window_id);
        self.root = Some(root);
        self.count = Some(count);
        self.window = Some(window.clone());
        self.app = Some(app);
        window.request_redraw();
        Ok(())
    }

    fn redraw(&mut self, event_loop: &ActiveEventLoop) {
        let (Some(window_id), Some(window)) = (self.window_id, self.window.clone()) else {
            return;
        };
        // Consume the request that caused this redraw so an occluded surface cannot keep
        // coalescing later input into a request that was never presented.
        // 消费触发本次重绘的请求，避免被遮挡的 Surface 把后续输入持续合并到未呈现请求中。
        if let Some(context) = self.app().window(window_id) {
            let _ = context.take_frame_request();
        }
        let result = self.app().render(window_id);
        let advance = match result {
            Ok(RenderStatus::Rendered) => {
                self.surface_presented = true;
                true
            }
            Ok(RenderStatus::Reconfigured) => {
                window.request_redraw();
                return;
            }
            Ok(RenderStatus::SurfaceLost) => {
                if let Err(error) = self.app().attach_surface(window_id, window.clone()) {
                    self.fail(error.to_string(), event_loop);
                    return;
                }
                window.request_redraw();
                return;
            }
            Ok(RenderStatus::Skipped) => {
                // An occluded or timed-out surface cannot present, but a smoke run still
                // drives the same window-tagged input path and reports that no frame was shown.
                // 被遮挡或超时的 Surface 无法呈现，但 smoke 运行仍走同一窗口输入路径，
                // 并明确报告没有帧被显示。
                self.smoke != SmokeState::Disabled
                    && self
                        .app()
                        .window(window_id)
                        .is_some_and(|context| context.surface().is_some())
            }
            Err(error) => {
                self.fail(error.to_string(), event_loop);
                return;
            }
        };
        if advance && self.smoke != SmokeState::Disabled {
            self.advance_smoke(event_loop, window_id, &window);
        }
    }

    fn advance_smoke(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: WindowId,
        window: &Arc<Window>,
    ) {
        let Some(root) = self.root else {
            self.fail("smoke run has no mounted root".to_owned(), event_loop);
            return;
        };
        let (text, bounds) = {
            let Some(context) = self.app().window(window_id) else {
                self.fail("smoke run lost its window context".to_owned(), event_loop);
                return;
            };
            (
                context.text_content(root).unwrap_or_default(),
                context.committed_bounds(window_id, root),
            )
        };
        match self.smoke {
            SmokeState::AwaitFirstFrame => {
                if text != "Count: 0" {
                    self.fail(
                        format!("expected Count: 0 on first frame, got {text:?}"),
                        event_loop,
                    );
                    return;
                }
                self.smoke_initial_text = Some(text.clone());
                let Some(bounds) = bounds else {
                    self.fail("smoke run has no button bounds".to_owned(), event_loop);
                    return;
                };
                let center = bounds.center();
                for action in [PointerAction::Pressed, PointerAction::Released] {
                    let _ = self.app().route_input(InputEvent::Pointer(PointerInput {
                        window: window_id,
                        position: center,
                        action,
                    }));
                }
                for action in [PointerAction::Pressed, PointerAction::Released] {
                    let _ = self.app().route_input(InputEvent::Pointer(PointerInput {
                        window: window_id,
                        position: center,
                        action,
                    }));
                }
                self.smoke = SmokeState::AwaitSecondFrame;
                window.request_redraw();
            }
            SmokeState::AwaitSecondFrame => {
                if text != "Count: 2" {
                    self.fail(
                        format!("expected Count: 2 after two clicks, got {text:?}"),
                        event_loop,
                    );
                    return;
                }
                let (signals, stats) = {
                    let Some(context) = self.app().window(window_id) else {
                        return;
                    };
                    (context.signals().snapshot(), self.app().shared().stats())
                };
                println!(
                    "{}",
                    serde_json::to_string_pretty(&json!({
                        "native_smoke": "passed",
                        "window": window_id,
                        "initial_text": self.smoke_initial_text,
                        "text": text,
                        "surface_presented": self.surface_presented,
                        "frame_requests": signals.frame_requests,
                        "committed_frames": signals.committed_frames,
                        "rasterized_glyphs": stats.rasterized_glyphs,
                        "vertices": stats.vertices,
                    }))
                    .expect("native smoke JSON")
                );
                self.smoke = SmokeState::Done;
                event_loop.exit();
            }
            SmokeState::Disabled | SmokeState::Done => {}
        }
    }

    fn fail(&mut self, message: String, event_loop: &ActiveEventLoop) {
        self.failure = Some(message);
        event_loop.exit();
    }

    fn route_pointer(&mut self, action: PointerAction) {
        let Some(window_id) = self.window_id else {
            return;
        };
        let position = self.pointer;
        let _ = self.app().route_input(InputEvent::Pointer(PointerInput {
            window: window_id,
            position,
            action,
        }));
    }

    fn resize(&mut self, window: &Arc<Window>, size: winit::dpi::PhysicalSize<u32>) {
        if size.width == 0 || size.height == 0 {
            return;
        }
        let Some(window_id) = self.window_id else {
            return;
        };
        let dpr = window.scale_factor() as f32;
        let logical: LogicalSize<f32> = size.to_logical(dpr as f64);
        let _ = self.app().route_input(InputEvent::Resize {
            window: window_id,
            viewport: Size::new(logical.width, logical.height),
            dpr,
        });
        window.request_redraw();
    }
}

/// Applies the real window geometry and DPR before the first surface configuration.
/// 在首次配置 Surface 前应用真实窗口几何与 DPR。
fn apply_initial_viewport(
    context: &mut WindowContext,
    physical: PhysicalSize<u32>,
    scale_factor: f64,
) {
    let logical = physical.to_logical::<f32>(scale_factor);
    context.resize(
        Size::new(logical.width.max(1.0), logical.height.max(1.0)),
        scale_factor as f32,
    );
}

impl ApplicationHandler for CounterHost {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if let Err(error) = self.ensure_window(event_loop) {
            self.fail(error, event_loop);
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: WinitWindowId,
        event: WinitWindowEvent,
    ) {
        if self.winit_window != Some(window_id) {
            return;
        }
        let Some(window) = self.window.clone() else {
            return;
        };
        match event {
            WinitWindowEvent::CloseRequested => event_loop.exit(),
            WinitWindowEvent::RedrawRequested => self.redraw(event_loop),
            WinitWindowEvent::Resized(size) => self.resize(&window, size),
            WinitWindowEvent::ScaleFactorChanged { .. } => {
                self.resize(&window, window.inner_size());
            }
            WinitWindowEvent::Occluded(false) => window.request_redraw(),
            WinitWindowEvent::Focused(focused) => {
                if let Some(window_id) = self.window_id {
                    let _ = self.app().route_input(InputEvent::Focus {
                        window: window_id,
                        focused,
                    });
                }
            }
            WinitWindowEvent::CursorMoved { position, .. } => {
                let dpr = window.scale_factor() as f32;
                let logical = position.to_logical::<f32>(dpr as f64);
                self.pointer = fino::native::Point::new(logical.x, logical.y);
            }
            WinitWindowEvent::CursorLeft { .. } => {
                self.route_pointer(PointerAction::Cancelled);
            }
            WinitWindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: MouseButton::Left,
                ..
            } => self.route_pointer(PointerAction::Pressed),
            WinitWindowEvent::MouseInput {
                state: ElementState::Released,
                button: MouseButton::Left,
                ..
            } => self.route_pointer(PointerAction::Released),
            WinitWindowEvent::Ime(Ime::Commit(text)) => {
                if let Some(window_id) = self.window_id {
                    let _ = self.app().route_input(InputEvent::Ime {
                        window: window_id,
                        event: fino::native::ImeEvent::Commit(text),
                    });
                }
            }
            WinitWindowEvent::KeyboardInput { event, .. }
                if event.state == ElementState::Pressed =>
            {
                if event.logical_key == Key::Named(NamedKey::Escape) {
                    event_loop.exit();
                } else {
                    let key = match &event.logical_key {
                        Key::Named(NamedKey::Enter) => "Enter",
                        Key::Named(NamedKey::Space) => " ",
                        _ => return,
                    };
                    if let Some(window_id) = self.window_id {
                        let _ = self.app().route_input(InputEvent::Key(KeyInput {
                            window: window_id,
                            key: key.to_owned(),
                            pressed: true,
                            repeat: event.repeat,
                        }));
                    }
                }
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        let Some(window_id) = self.window_id else {
            return;
        };
        let Some(window) = self.window.clone() else {
            return;
        };
        let Some(context) = self.app().window(window_id) else {
            return;
        };
        if context.take_frame_request() {
            window.request_redraw();
        }
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::rc::Rc;

    use fino::{
        native::{Size, WindowContext, WindowDesc, WindowId},
        text::TextEngine,
    };

    use super::{PhysicalSize, apply_initial_viewport};

    /// The first surface configuration must use the real physical size and scale factor.
    /// 首次 Surface 配置必须使用真实物理尺寸与缩放因子。
    #[test]
    fn initial_viewport_uses_physical_size_and_scale_factor() {
        let mut context = WindowContext::new(
            WindowId::new(1),
            WindowDesc::default(),
            Rc::new(RefCell::new(TextEngine::new())),
        );
        apply_initial_viewport(&mut context, PhysicalSize::new(1000, 600), 2.0);
        assert_eq!(context.viewport(), Size::new(500.0, 300.0));
        assert_eq!(context.dpr(), 2.0);
    }
}
